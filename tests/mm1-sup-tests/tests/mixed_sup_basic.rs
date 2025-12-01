use std::time::Duration;

use mm1::common::log;
use mm1::core::context::{Fork, InitDone, Linking, Messaging, Now, Quit, Start, Stop, Watching};
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};
use mm1::sup::common::{ActorFactoryMut, ChildSpec, InitType, RestartIntensity};
use mm1::sup::mixed::strategy::OneForOne;
use mm1::sup::mixed::{self, ChildType, MixedSup};
use tokio::time::Instant;

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "mm1_sup::*=DEBUG".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}

#[test]
fn test_01() {
    let _ = mm1_logger::init(&logger_config());

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(local::boxed_from_fn(main))
        .expect("Rt::run");
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Now<Instant = Instant>,
    Ctx: Fork + Messaging + Quit + InitDone + Linking + Watching + Start<Local> + Stop,
{
    let sup = MixedSup::new(OneForOne::new(RestartIntensity {
        max_restarts: 3,
        within:       Duration::from_secs(30),
    }))
    .with_child(
        "w1".to_string(),
        ChildSpec::new(ActorFactoryMut::new(|()| local::boxed_from_fn(w1)))
            .with_child_type(ChildType::Permanent)
            .with_init_type(InitType::NoAck)
            .with_stop_timeout(Duration::from_secs(3)),
    )
    .with_child(
        "w2".to_string(),
        ChildSpec::new(ActorFactoryMut::new(|()| local::boxed_from_fn(w2)))
            .with_child_type(ChildType::Permanent)
            .with_init_type(InitType::WithAck {
                start_timeout: Duration::from_secs(1),
            })
            .with_stop_timeout(Duration::from_secs(3)),
    )
    .with_child(
        "w3".to_string(),
        ChildSpec::new(ActorFactoryMut::new(|()| local::boxed_from_fn(w3)))
            .with_child_type(ChildType::Permanent)
            .with_init_type(InitType::NoAck)
            .with_stop_timeout(Duration::from_secs(3)),
    );

    mixed::mixed_sup(ctx, sup)
        .await
        .expect("ew: top sup failure");
}

async fn w1<Ctx>(_ctx: &mut Ctx) {
    log::info!("hello!");
}

async fn w2<Ctx>(_ctx: &mut Ctx)
where
    Ctx: Messaging + InitDone,
{
    log::info!("how do you do?");
    // ctx.init_done(ctx.address()).await;
    // panic!("bye!")
}

async fn w3<Ctx>(_ctx: &mut Ctx) {
    log::info!("hi how are you!")
}
