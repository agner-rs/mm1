//! Repro for #134: `dispatch!` on an unmatched message must not crash the
//! actor.
//!
//! An actor whose `dispatch!` has no catch-all arm receives a message of an
//! unexpected type, then a normal request. It must survive the unexpected
//! message and answer the request. Today the generated fallback is `panic!`, so
//! the actor dies on the unexpected message and its linked, exit-trapping
//! parent observes an `Exited { normal_exit: false }` instead of the reply.

use std::time::Duration;

use futures::FutureExt;
use mm1::address::Address;
use mm1::core::context::{InitDone, Linking, Messaging, Start, Tell};
use mm1::core::envelope::dispatch;
use mm1::proto::message;
use mm1::proto::system::Exited;
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};
use tokio::sync::oneshot;

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![LogTargetConfig {
            path:  vec!["*".into()],
            level: Level::TRACE,
        }],
    }
}

#[derive(Debug)]
#[message]
struct Ping {
    reply_to: Address,
}

#[derive(Debug)]
#[message]
struct Pong;

#[derive(Debug)]
#[message]
struct Unexpected;

#[test]
fn dispatch_survives_unmatched_message() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<bool>();

    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<bool>)
    where
        C: Messaging + Linking + Start<Local> + Tell,
    {
        // Trap exits and link to the survivor, so its death (if it panics on the
        // unexpected message) arrives here as an `Exited`.
        ctx.set_trap_exit(true).await;
        let survivor = ctx
            .start(local::boxed_from_fn(survivor), true, Duration::from_secs(1))
            .await
            .expect("start survivor");

        // An unexpected message type first — today this panics the actor.
        let _ = ctx.tell(survivor, Unexpected).await;
        // Then a normal request. Delivered after `Unexpected` (same mailbox, FIFO).
        let _ = ctx
            .tell(
                survivor,
                Ping {
                    reply_to: ctx.address(),
                },
            )
            .await;

        // Exactly one of these arrives: `Pong` (the survivor lived and answered)
        // or `Exited` (it died on the unexpected message).
        dispatch!(match ctx.recv().await.expect("recv") {
            Pong => {
                let _ = tx.send(true);
            },
            _exited @ Exited { .. } => {
                let _ = tx.send(false);
            },
        });
    }

    async fn survivor<C>(ctx: &mut C)
    where
        C: Messaging + InitDone + Tell,
    {
        ctx.init_done(ctx.address()).await;
        loop {
            dispatch!(match ctx.recv().await.expect("recv") {
                Ping { reply_to } => {
                    let _ = ctx.tell(reply_to, Pong).await;
                },
            });
        }
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");

    let survived = rx
        .now_or_never()
        .expect("main did not report an outcome")
        .expect("rx");
    assert!(
        survived,
        "#134: an actor must survive an unmatched message (log-and-drop), not panic and die"
    );
}
