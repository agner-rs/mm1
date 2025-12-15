use std::time::Duration;

use mm1::common::error::HasErrorKind;
use mm1::common::log::info;
use mm1::core::context::{Fork, InitDone, Messaging, Ping, Start, Stop, Watching};
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};

#[test]
fn test_ping_local_actor() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Start<Local> + Messaging + Ping,
    {
        let responder = ctx
            .start(
                local::boxed_from_fn(responder),
                true,
                Duration::from_millis(100),
            )
            .await
            .expect("start responder");

        info!(%responder, "started responder");

        let duration = ctx
            .ping(responder, Duration::from_secs(5))
            .await
            .expect("ping responder");

        info!("Ping took: {:?}", duration);
        assert!(duration < Duration::from_secs(1));
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(local::boxed_from_fn(main))
        .expect("Rt::run");
}

#[test]
fn test_ping_timeout() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Fork + Start<Local> + Messaging + Ping + Stop + Watching,
    {
        let responder = ctx
            .start(
                local::boxed_from_fn(responder),
                false,
                Duration::from_millis(100),
            )
            .await
            .expect("start responder");

        tokio::time::sleep(Duration::from_millis(10)).await;

        ctx.shutdown(responder, Duration::from_secs(1))
            .await
            .expect("failed to shutdown responder");

        let result = ctx.ping(responder, Duration::from_secs(1)).await;

        assert!(result.is_err(), "Expected ping to timeout but it succeeded");
        if let Err(e) = result {
            assert_eq!(
                e.kind(),
                mm1::core::context::PingErrorKind::Timeout,
                "Expected Timeout error, got: {:?}",
                e.kind()
            );
        }
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(local::boxed_from_fn(main))
        .expect("Rt::run");
}

#[test]
fn test_ping_multiple_actors() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Start<Local> + Messaging + Ping,
    {
        let responder1 = ctx
            .start(
                local::boxed_from_fn(responder),
                false,
                Duration::from_millis(100),
            )
            .await
            .expect("start responder1");

        let responder2 = ctx
            .start(
                local::boxed_from_fn(responder),
                false,
                Duration::from_millis(100),
            )
            .await
            .expect("start responder2");

        let responder3 = ctx
            .start(
                local::boxed_from_fn(responder),
                false,
                Duration::from_millis(100),
            )
            .await
            .expect("start responder3");

        let duration1 = ctx
            .ping(responder1, Duration::from_secs(1))
            .await
            .expect("ping responder1");
        let duration2 = ctx
            .ping(responder2, Duration::from_secs(1))
            .await
            .expect("ping responder2");
        let duration3 = ctx
            .ping(responder3, Duration::from_secs(1))
            .await
            .expect("ping responder3");

        info!(
            "Pings took: {:?}, {:?}, {:?}",
            duration1, duration2, duration3
        );
        assert!(duration1 < Duration::from_secs(1));
        assert!(duration2 < Duration::from_secs(1));
        assert!(duration3 < Duration::from_secs(1));
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(local::boxed_from_fn(main))
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

async fn responder<C>(ctx: &mut C)
where
    C: InitDone + Messaging,
{
    ctx.init_done(ctx.address()).await;

    loop {
        let _ = ctx.recv().await;
    }
}
