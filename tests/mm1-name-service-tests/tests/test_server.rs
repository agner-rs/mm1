use std::sync::Arc;
use std::time::Duration;

use mm1::address::{AddressPool, NetAddress, NetMask};
use mm1::ask::proto::{Request, RequestHeader, Response};
use mm1::core::context::{Bind, Fork, InitDone, Messaging, Now, Quit};
use mm1::core::envelope::{Envelope, EnvelopeHeader};
use mm1::test::rt::event::{EventResolve, EventResolveResult};
use mm1::test::rt::{TestRuntime, query};
use mm1_proto_named::{
    KeyProps, RegProps, RegisterRequest, RegisterResponse, ResolveRequest, ResolveResponse,
};
use mm1_proto_well_known::NAME_SERVICE;
use tokio::time::{self, Instant};

#[tokio::test]
async fn test_simple_actor() {
    time::pause();

    let rt = TestRuntime::<()>::new();

    let address_pool = AddressPool::new("<cafe:>/16".parse().unwrap());
    let server_lease = address_pool.lease(NetMask::M_64).unwrap();
    let server_address = server_lease.address;

    let client_lease = address_pool.lease(NetMask::M_64).unwrap();
    let client_address = client_lease.address;

    rt.add_actor(server_address, Some(server_lease), launcher)
        .await
        .unwrap();

    let bind = rt.expect_next_event().await.expect::<query::Bind<_>>();
    let bind_to = bind.args.bind_to;
    assert_eq!(
        bind_to,
        NetAddress {
            address: NAME_SERVICE,
            mask:    NetMask::M_64,
        }
    );
    bind.resolve_ok(());

    let init_done = rt.expect_next_event().await.expect::<query::InitDone>();
    assert_eq!(init_done.address, init_done.task_key.actor);
    init_done.resolve(());

    // resolve unregistered
    let recv = rt.expect_next_event().await.expect::<query::Recv>();

    recv.resolve_ok({
        let header = RequestHeader {
            id:       Default::default(),
            reply_to: client_address,
        };
        let payload = ResolveRequest {
            key: Arc::<str>::from("service-name"),
        };
        let request = Request { header, payload };
        Envelope::new(EnvelopeHeader::to_address(NAME_SERVICE), request).into_erased()
    });

    let mut tell = rt.expect_next_event().await.expect::<query::Tell>();
    let (response, envelope) = tell
        .take_envelope()
        .cast::<Response<ResolveResponse>>()
        .unwrap()
        .take();
    tell.resolve_ok(());

    assert_eq!(envelope.header().to, client_address);
    assert!(response.payload.unwrap().is_empty());

    // register
    let reg_1_lease = address_pool.lease(NetMask::M_64).unwrap();
    let reg_1_address = reg_1_lease.address;
    let recv = rt.expect_next_event().await.expect::<query::Recv>();
    recv.resolve_ok({
        let header = RequestHeader {
            id:       Default::default(),
            reply_to: client_address,
        };
        let payload = RegisterRequest {
            key:       Arc::<str>::from("service-name"),
            addr:      reg_1_address,
            key_props: KeyProps { exclusive: true },
            reg_props: RegProps {
                ttl: Duration::from_secs(60),
            },
        };
        let request = Request { header, payload };
        Envelope::new(EnvelopeHeader::to_address(NAME_SERVICE), request).into_erased()
    });
    let mut tell = rt.expect_next_event().await.expect::<query::Tell>();
    let (response, envelope) = tell
        .take_envelope()
        .cast::<Response<RegisterResponse>>()
        .unwrap()
        .take();
    tell.resolve_ok(());

    assert_eq!(envelope.header().to, client_address);
    response.payload.unwrap();

    // resolve registered
    time::advance(Duration::from_secs(30)).await;

    let recv = rt.expect_next_event().await.expect::<query::Recv>();

    recv.resolve_ok({
        let header = RequestHeader {
            id:       Default::default(),
            reply_to: client_address,
        };
        let payload = ResolveRequest {
            key: Arc::<str>::from("service-name"),
        };
        let request = Request { header, payload };
        Envelope::new(EnvelopeHeader::to_address(NAME_SERVICE), request).into_erased()
    });

    let mut tell = rt.expect_next_event().await.expect::<query::Tell>();
    let (response, envelope) = tell
        .take_envelope()
        .cast::<Response<ResolveResponse>>()
        .unwrap()
        .take();
    tell.resolve_ok(());

    assert_eq!(envelope.header().to, client_address);
    assert_eq!(
        response.payload.unwrap().as_ref(),
        [(reg_1_address, Duration::from_secs(30))]
    );

    // resolve ttl-ed
    time::advance(Duration::from_secs(30)).await;

    let recv = rt.expect_next_event().await.expect::<query::Recv>();

    recv.resolve_ok({
        let header = RequestHeader {
            id:       Default::default(),
            reply_to: client_address,
        };
        let payload = ResolveRequest {
            key: Arc::<str>::from("service-name"),
        };
        let request = Request { header, payload };
        Envelope::new(EnvelopeHeader::to_address(NAME_SERVICE), request).into_erased()
    });

    let mut tell = rt.expect_next_event().await.expect::<query::Tell>();
    let (response, envelope) = tell
        .take_envelope()
        .cast::<Response<ResolveResponse>>()
        .unwrap()
        .take();
    tell.resolve_ok(());

    assert_eq!(envelope.header().to, client_address);
    assert!(response.payload.unwrap().is_empty());

    async fn launcher<C>(ctx: &mut C)
    where
        C: Bind<NetAddress>,
        C: Quit + Now<Instant = Instant> + Fork + Messaging + InitDone,
    {
        mm1_name_service::server::name_server_actor(ctx, [NAME_SERVICE.into()])
            .await
            .expect("service failed");
    }
}
