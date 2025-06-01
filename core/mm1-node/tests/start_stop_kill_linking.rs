use std::time::Duration;

use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_core::context::{Fork, InitDone, Linking, Messaging, Start, Stop, Watching};
use mm1_node::runtime::{Local, Rt};
use mm1_proto_system::{Down, Exited};

#[test]
fn test_linking_with_trap_exit() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Start<Local> + Linking + Stop + Messaging,
    {
        ctx.set_trap_exit(true).await;

        let quick = ctx
            .start(Local::actor(quick), true, Duration::from_millis(1))
            .await
            .expect("start quick");
        let obedient = ctx
            .start(Local::actor(obedient), true, Duration::from_millis(1))
            .await
            .expect("start obedient");
        let stubborn = ctx
            .start(Local::actor(stubborn), true, Duration::from_millis(1))
            .await
            .expect("start stubborn");

        let (Exited { peer, .. }, _) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!(peer, quick);
        assert!(!ctx.exit(quick).await);

        ctx.exit(obedient).await;
        let (Exited { peer, .. }, _) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!(peer, obedient);

        ctx.exit(stubborn).await;
        ctx.recv()
            .timeout(Duration::from_millis(1))
            .await
            .expect_err("stubborn shouldn't have exited");

        ctx.kill(stubborn).await;
        let (Exited { peer, .. }, _) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!(peer, stubborn);
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(Local::actor(main))
        .expect("Rt::run");
}

#[test]
fn test_watching_no_trap_exit() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Start<Local> + Watching + Stop + Messaging,
    {
        let quick = ctx
            .start(Local::actor(quick), false, Duration::from_millis(1))
            .await
            .expect("start quick");
        let obedient = ctx
            .start(Local::actor(obedient), false, Duration::from_millis(1))
            .await
            .expect("start obedient");
        let stubborn = ctx
            .start(Local::actor(stubborn), false, Duration::from_millis(1))
            .await
            .expect("start stubborn");

        let quick_wref = ctx.watch(quick).await;
        let obedient_wref = ctx.watch(obedient).await;
        let stubborn_wref = ctx.watch(stubborn).await;

        let (
            Down {
                peer, watch_ref, ..
            },
            _,
        ) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!((peer, watch_ref), (quick, quick_wref));
        assert!(!ctx.exit(quick).await);

        ctx.exit(obedient).await;
        let (
            Down {
                peer, watch_ref, ..
            },
            _,
        ) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!((peer, watch_ref), (obedient, obedient_wref));

        ctx.exit(stubborn).await;
        ctx.recv()
            .timeout(Duration::from_millis(1))
            .await
            .expect_err("stubborn shouldn't have exited");

        ctx.kill(stubborn).await;
        let (
            Down {
                peer, watch_ref, ..
            },
            _,
        ) = ctx
            .recv()
            .timeout(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap()
            .cast()
            .unwrap()
            .take();
        assert_eq!((peer, watch_ref), (stubborn, stubborn_wref));
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(Local::actor(main))
        .expect("Rt::run");
}

#[test]
fn test_shutdown() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Fork + Messaging + Start<Local> + Watching + Stop,
    {
        let quick = ctx
            .start(Local::actor(quick), false, Duration::from_millis(1))
            .await
            .expect("start quick");
        let obedient = ctx
            .start(Local::actor(obedient), false, Duration::from_millis(1))
            .await
            .expect("start obedient");
        let stubborn = ctx
            .start(Local::actor(stubborn), false, Duration::from_millis(1))
            .await
            .expect("start stubborn");

        ctx.shutdown(quick, Duration::from_millis(10))
            .await
            .expect("shutdown quick");
        ctx.shutdown(obedient, Duration::from_millis(10))
            .await
            .expect("shutdown obedient");
        ctx.shutdown(stubborn, Duration::from_millis(10))
            .await
            .expect("shutdown stubborn");
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(Local::actor(main))
        .expect("Rt::run");
}

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

async fn quick<C>(ctx: &mut C)
where
    C: InitDone + Messaging,
{
    ctx.init_done(ctx.address()).await;
}

async fn obedient<C>(ctx: &mut C)
where
    C: InitDone + Messaging,
{
    ctx.init_done(ctx.address()).await;

    std::future::pending().await
}
async fn stubborn<C>(ctx: &mut C)
where
    C: InitDone + Messaging + Linking,
{
    ctx.set_trap_exit(true).await;
    ctx.init_done(ctx.address()).await;

    loop {
        let something_to_ignore = ctx.recv().await.expect("recv error");
        eprintln!("[{}] IGNORING: {:?}", ctx.address(), something_to_ignore);
    }
}
