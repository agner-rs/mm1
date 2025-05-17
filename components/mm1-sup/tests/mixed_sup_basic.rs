use std::time::Duration;

use mm1_common::log;
use mm1_core::context::{Fork, InitDone, Linking, Now, Quit, Recv, Start, Stop, Tell, Watching};
use mm1_node::runtime::{Local, Rt};
use mm1_sup::common::child_spec::{ChildSpec, ChildType, InitType};
use mm1_sup::common::factory::ActorFactoryMut;
use mm1_sup::common::restart_intensity::RestartIntensity;
use mm1_sup::mixed::strategy::OneForOne;
use mm1_sup::mixed::{self, MixedSup};
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
        .run(Local::actor(main))
        .expect("Rt::run");
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Now<Instant = Instant>,
    Ctx: Fork + Recv + Tell + Quit + InitDone + Linking + Watching + Start<Local> + Stop,
{
    let sup = MixedSup::new(OneForOne::new(RestartIntensity {
        max_restarts: 3,
        within:       Duration::from_secs(30),
    }))
    .with_child(
        "w1".to_string(),
        ChildSpec {
            launcher:     ActorFactoryMut::new(|()| Local::actor(w1)),
            child_type:   ChildType::Permanent,
            init_type:    InitType::NoAck,
            stop_timeout: Duration::from_secs(3),
        },
    )
    .with_child(
        "w2".to_string(),
        ChildSpec {
            launcher:     ActorFactoryMut::new(|()| Local::actor(w2)),
            child_type:   ChildType::Permanent,
            init_type:    InitType::WithAck {
                start_timeout: Duration::from_secs(1),
            },
            stop_timeout: Duration::from_secs(3),
        },
    )
    .with_child(
        "w3".to_string(),
        ChildSpec {
            launcher:     ActorFactoryMut::new(|()| Local::actor(w3)),
            child_type:   ChildType::Permanent,
            init_type:    InitType::NoAck,
            stop_timeout: Duration::from_secs(3),
        },
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
    Ctx: Recv + InitDone,
{
    log::info!("how do you do?");
    // ctx.init_done(ctx.address()).await;
    // panic!("bye!")
}

async fn w3<Ctx>(_ctx: &mut Ctx) {
    log::info!("hi how are you!")
}
