use std::time::Duration;

use futures::FutureExt;
use mm1::address::Address;
use mm1::core::context::{Fork, InitDone, Messaging, Start, Watching};
use mm1::core::envelope::dispatch;
use mm1::proto::system::{Down, WatchRef};
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};
use tokio::sync::oneshot;

#[test]
fn a_watch_can_be_cancelled_from_another_fork() {
    async fn target<C>(ctx: &mut C, exit_rx: oneshot::Receiver<()>)
    where
        C: InitDone + Messaging,
    {
        ctx.init_done(ctx.address()).await;
        exit_rx.await.expect("exit signal");
    }

    async fn main<C>(ctx: &mut C, observed_tx: oneshot::Sender<(Address, WatchRef, WatchRef, Down)>)
    where
        C: Fork + Messaging + Start<Local> + Watching,
    {
        let (exit_tx, exit_rx) = oneshot::channel();
        let target = ctx
            .start(
                local::boxed_from_fn((target, (exit_rx,))),
                false,
                Duration::from_secs(1),
            )
            .await
            .expect("start target");

        let cancelled_watch = ctx.watch(target).await;
        let sentinel_watch = ctx.watch(target).await;

        let (unwatched_tx, unwatched_rx) = oneshot::channel();
        ctx.fork()
            .await
            .expect("fork")
            .run(move |mut fork| {
                async move {
                    fork.unwatch(cancelled_watch).await;
                    unwatched_tx.send(()).expect("report unwatch completion");
                }
            })
            .await;
        unwatched_rx.await.expect("unwatch completion");

        exit_tx.send(()).expect("stop target");

        dispatch!(match ctx.recv().await.expect("target Down") {
            down @ Down { .. } =>
                observed_tx
                    .send((target, cancelled_watch, sentinel_watch, down))
                    .expect("report observed Down"),
            unexpected @ _ => panic!("unexpected message: {unexpected:?}"),
        });
    }

    let (observed_tx, observed_rx) = oneshot::channel();
    Rt::create(Default::default())
        .expect("create runtime")
        .run(local::boxed_from_fn((main, (observed_tx,))))
        .expect("run runtime");

    let (target, cancelled_watch, sentinel_watch, down) = observed_rx
        .now_or_never()
        .expect("main actor did not finish")
        .expect("main actor did not report a Down message");
    assert_eq!(down.peer, target);
    assert_ne!(down.watch_ref, cancelled_watch);
    assert_eq!(down.watch_ref, sentinel_watch);
    assert!(down.normal_exit);
}
