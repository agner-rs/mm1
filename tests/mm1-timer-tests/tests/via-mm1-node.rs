use std::time::Duration;

use mm1::common::log;
use mm1::core::context::{Fork, Messaging, Now, Quit};
use mm1::core::envelope::dispatch;
use mm1::runnable::local;
use mm1::runtime::Rt;
use mm1::timer::v1::OneshotTimer;
use tokio::task;
use tokio::time::Instant;

const INITIAL_DELAY: Duration = Duration::from_millis(200);
const STEP: Duration = Duration::from_millis(25);

#[test]
fn test() {
    let _ = mm1_logger::init(&logger_config());
    Rt::create(Default::default())
        .unwrap()
        .run(local::boxed_from_fn(main))
        .unwrap();
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Now<Instant = Instant> + Quit + Fork + Messaging,
{
    log::info!("HELLO!");

    let mut timers = OneshotTimer::create(ctx)
        .await
        .expect("could not create timer");

    timers
        .schedule_once_after(INITIAL_DELAY, INITIAL_DELAY)
        .await
        .unwrap();

    loop {
        let envelope = ctx.recv().await.unwrap();
        let d = dispatch!(match envelope {
            d @ Duration { .. } => d,
        });
        log::info!(d = ?d, "TICK");

        let Some(d) = d.checked_sub(STEP) else { break };

        timers.schedule_once_after(d, d).await.unwrap();
        timers.schedule_once_after(d, d).await.unwrap();

        task::yield_now().await;
    }

    log::info!("BYE!");
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "mm1_timer::*=DEBUG".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
