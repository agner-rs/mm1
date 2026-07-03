//! Repro for #132: `ctx.fork()` is not cancellation-safe.
//!
//! `fork()` inserts an entry into `fork_entries` and then `await`s `ForkAdded`.
//! If the future is dropped at that await, today the entry is leaked (the
//! cleanup lives in `ActorContext::drop`, which never ran). A later `fork()`
//! that leases the recycled address then hits
//! `assert!(should_be_none.is_none())` and panics the actor.
//!
//! The actor cancels one fork and then forks in a loop, keeping each fork
//! alive, until its address pool is exhausted — which eventually reuses the
//! freed address. A cancellation-safe fork never panics (it reuses the address
//! cleanly or just runs out of addresses), so the actor always reaches its
//! report; the buggy runtime panics on the reuse and the report never arrives.
//!
//! The default config is used deliberately: it gives every actor the same pool
//! size, so the extra service actors that appear under workspace-wide feature
//! unification (name-service, network management) are unaffected.

use futures::FutureExt;
use mm1::core::context::Fork;
use mm1::runnable::local;
use mm1::runtime::Rt;
use tokio::sync::oneshot;

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    // Quiet: the loop performs a few hundred forks; TRACE would bury the test.
    LoggingConfig {
        min_log_level:     Level::ERROR,
        log_target_filter: vec![LogTargetConfig {
            path:  vec!["*".into()],
            level: Level::ERROR,
        }],
    }
}

#[test]
fn fork_survives_a_cancelled_fork() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<usize>();

    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<usize>)
    where
        C: Fork,
    {
        // 1. Start a fork and cancel it at its first suspension point (the `ForkAdded`
        //    await), after it has leased its address and inserted the fork entry.
        {
            let mut forking = std::pin::pin!(ctx.fork());
            let polled = futures::poll!(forking.as_mut());
            assert!(
                polled.is_pending(),
                "fork() should suspend at the ForkAdded await on its first poll"
            );
            // Leaving this block drops (cancels) the fork future at that await.
        }

        // 2. Fork repeatedly, keeping each fork alive, until the pool is exhausted.
        //    This eventually leases the cancelled fork's recycled address. On the buggy
        //    runtime its fork-entry insert hits the leaked entry and the actor panics;
        //    a cancellation-safe fork either reuses the address cleanly or simply runs
        //    out of addresses — it never panics, so we always reach the report below.
        //    The bound comfortably exceeds the pool size (a safety net against an
        //    infinite loop).
        let mut forks = Vec::new();
        for _ in 0..4096 {
            match ctx.fork().await {
                Ok(fork) => forks.push(fork),
                Err(_exhausted) => break,
            }
        }

        let _ = tx.send(forks.len());
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    let _ = rt.run(local::boxed_from_fn((main, (tx,))));

    assert!(
        matches!(rx.now_or_never(), Some(Ok(_))),
        "#132: forking after a cancelled fork must not panic the actor"
    );
}
