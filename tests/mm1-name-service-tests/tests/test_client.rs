use std::sync::Arc;
use std::time::Duration;

use mm1::address::NetAddress;
use mm1::ask::Ask;
use mm1::core::context::{Bind, Fork, InitDone, Messaging, Quit, Start};
use mm1::runnable::local;
use mm1::runnable::local::BoxedRunnable;
use mm1::runtime::Rt;
use mm1_name_service::api::{Registration, Resolver};
use mm1_proto_well_known::NAME_SERVICE;
use tokio::time;

#[test]
fn test() {
    let _ = mm1_logger::init(&logger_config());

    let rt = Rt::create(Default::default()).unwrap();
    rt.run(local::boxed_from_fn(main_actor)).unwrap();
}

async fn main_actor<C>(ctx: &mut C)
where
    C: Messaging + Fork + Bind<NetAddress> + Start<BoxedRunnable<C>> + InitDone + Quit + Sync,
{
    ctx.start(
        local::boxed_from_fn((
            mm1_name_service::server::name_server_actor,
            ([NAME_SERVICE.into()],),
        )),
        true,
        Duration::from_millis(100),
    )
    .await
    .unwrap();

    let service_a = ctx
        .start(
            local::boxed_from_fn((service_actor, ("service-a", Duration::ZERO))),
            true,
            Duration::from_millis(10),
        )
        .await
        .unwrap();
    let service_b_0 = ctx
        .start(
            local::boxed_from_fn((service_actor, ("service-b", Duration::from_millis(50)))),
            true,
            Duration::from_millis(10),
        )
        .await
        .unwrap();
    let service_b_1 = ctx
        .start(
            local::boxed_from_fn((service_actor, ("service-b", Duration::from_millis(50)))),
            true,
            Duration::from_millis(10),
        )
        .await
        .unwrap();

    let mut resolver = Resolver::new(ctx.fork().await.unwrap(), NAME_SERVICE);

    let mut resolution_a = resolver.resolve("service-a").await.unwrap();
    let mut resolution_b = resolver.resolve("service-b").await.unwrap();
    let mut resolution_c = resolver.resolve("service-c").await.unwrap();

    assert_eq!(resolution_a.any(), None);
    assert_eq!(resolution_b.any(), None);

    resolution_a
        .wait(&mut resolver, 10, Duration::from_millis(10))
        .await
        .unwrap();
    assert_eq!(resolution_a.any(), Some(service_a));

    resolution_b
        .wait(&mut resolver, 10, Duration::from_millis(10))
        .await
        .unwrap();
    assert!(resolution_b.first() == Some(service_b_0) || resolution_b.first() == Some(service_b_1));

    resolution_c
        .wait(&mut resolver, 10, Duration::from_millis(10))
        .await
        .unwrap_err();
}

async fn service_actor<C>(ctx: &mut C, name: &'static str, registration_delay: Duration)
where
    C: Fork + Ask + InitDone,
{
    ctx.init_done(ctx.address()).await;

    time::sleep(registration_delay).await;

    let key = Arc::<str>::from(name);

    Registration::new(ctx.fork().await.unwrap(), NAME_SERVICE, key, ctx.address())
        .with_exclusive(false)
        .with_refresh_interval(Duration::from_millis(100))
        .with_ttl(Duration::from_millis(150))
        .with_register_timeout(Duration::from_millis(10))
        .run()
        .await
        .unwrap();
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            "mm1_name_service::*=TRACE".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
