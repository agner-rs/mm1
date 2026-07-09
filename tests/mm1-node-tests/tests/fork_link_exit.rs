//! Repro for #149: a fork's end notifies watchers but not linked peers.
//!
//! An actor traps exits, forks a sub-fork, links to it, and lets the fork end
//! normally. A trapping peer linked to a fork must observe the fork's end the
//! same way a watcher gets a `Down` — as `Exited { normal_exit: true }`. Today
//! the live-actor `ForkDone` handler only sends a bare `Disconnect` to linked
//! peers, so nothing is delivered and the peer waits forever (detected here
//! with a bounded timeout).

use std::time::Duration;

use futures::FutureExt;
use mm1::core::context::{Fork, Linking, Messaging};
use mm1::core::envelope::dispatch;
use mm1::proto::system::Exited;
use mm1::runnable::local;
use mm1::runtime::Rt;
use tokio::sync::oneshot;

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::ERROR,
        log_target_filter: vec![LogTargetConfig {
            path:  vec!["*".into()],
            level: Level::ERROR,
        }],
    }
}

#[test]
fn fork_end_notifies_a_trapping_linked_peer() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<Option<bool>>();

    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<Option<bool>>)
    where
        C: Fork + Linking + Messaging,
    {
        ctx.set_trap_exit(true).await;

        // Fork a sub-fork, link to it, then let it end normally.
        let fork = ctx.fork().await.expect("fork");
        let fork_addr = fork.address();
        ctx.link(fork_addr).await;
        fork.run(|_fork_ctx| async {}).await;

        // A trapping peer linked to the fork must observe its normal end as
        // `Exited { normal_exit: true }`. On the buggy runtime the fork end only
        // sends a bare `Disconnect`, so nothing arrives and we time out.
        let outcome = match tokio::time::timeout(Duration::from_secs(2), ctx.recv()).await {
            Ok(Ok(envelope)) => {
                dispatch!(match envelope {
                    Exited { peer, normal_exit } if *peer == fork_addr => Some(normal_exit),
                    other @ _ => panic!("unexpected message: {other:?}"),
                })
            },
            _ => None,
        };

        let _ = tx.send(outcome);
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    let _ = rt.run(local::boxed_from_fn((main, (tx,))));

    let outcome = rx
        .now_or_never()
        .expect("main did not report an outcome")
        .expect("rx");
    assert_eq!(
        outcome,
        Some(true),
        "#149: a fork's normal end must reach a trapping linked peer as Exited {{ normal_exit: \
         true }}"
    );
}
