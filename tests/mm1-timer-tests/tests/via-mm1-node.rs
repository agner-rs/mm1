#![cfg(feature = "tokio-time")]

use std::time::Duration;

use mm1_common::log;
use mm1_core::context::{Fork, Messaging, Now, Quit};
use mm1_core::envelope::dispatch;
use mm1_node::runtime::Rt;
use mm1_runnable::local;
use mm1_timer::api::TimerApi;
use tokio::task;
use tokio::time::Instant;

const INITIAL_DELAY: Duration = Duration::from_millis(100);
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

    let mut next_key = 1u32;
    let mut timers = mm1_timer::new_tokio_timer::<u32, Duration, _>(ctx)
        .await
        .unwrap();

    timers
        .schedule_once_after(next_key, INITIAL_DELAY, INITIAL_DELAY)
        .await
        .unwrap();
    next_key += 1;

    loop {
        let envelope = ctx.recv().await.unwrap();
        let d = dispatch!(match envelope {
            d @ Duration { .. } => d,
        });
        log::info!("TICK: {:?}", d);

        let Some(d) = d.checked_sub(STEP) else { break };

        timers.schedule_once_after(next_key, d, d).await.unwrap();
        next_key += 1;
        timers.schedule_once_after(next_key, d, d).await.unwrap();
        next_key += 1;

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
