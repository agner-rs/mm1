#![cfg(feature = "tokio-time")]

use std::time::Duration;

use mm1_address::address::Address;
use mm1_address::pool::Pool;
use mm1_address::subnet::NetMask;
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_proto_timer as t;
use mm1_test_rt::rt::event::EventResolveResult;
use mm1_test_rt::rt::{Runtime, query};
use mm1_timer::tokio_time::TokioTimer;
use tokio::time;
use tokio::time::Instant;

#[tokio::test]
async fn test() {
    let _ = mm1_logger::init(&logger_config());

    time::pause();

    type T = TokioTimer<&'static str, i32>;

    let rt = Runtime::<&'static str>::new();
    let address_pool = Pool::new("<ff:>/32".parse().unwrap());
    let main_lease = address_pool.lease(NetMask::M_64).unwrap();
    let timer_lease = address_pool.lease(NetMask::M_64).unwrap();
    rt.add_actor(
        Address::from_u64(1),
        Some(timer_lease),
        (mm1_timer::actor::timer_actor::<T, _>, (main_lease.address,)),
    )
    .await
    .unwrap();

    let recv = dbg!(
        rt.expect_next_event()
            .await
            .convert::<query::Recv>()
            .unwrap()
    );
    recv.resolve_ok(
        Envelope::new(
            EnvelopeHeader::to_address(main_lease.address),
            t::ScheduleOnce::<T> {
                key: "one",
                at:  Instant::now() + Duration::from_secs(1),
                msg: 1,
            },
        )
        .into_erased(),
    );

    time::sleep(Duration::from_secs(1)).await;

    let _recv = dbg!(
        rt.expect_next_event()
            .await
            .convert::<query::Recv>()
            .unwrap()
    );
    let mut tell = dbg!(
        rt.expect_next_event()
            .await
            .convert::<query::Tell>()
            .unwrap()
    );

    let envelope = tell.take_envelope();
    tell.resolve_ok(());
    assert_eq!(envelope.header().to, main_lease.address);
    let (msg, _) = envelope.cast::<i32>().unwrap().take();
    assert_eq!(msg, 1);
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "mm1_timer::*=TRACE".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
