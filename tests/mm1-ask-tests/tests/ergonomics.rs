use std::time::Duration;

use mm1::address::{Address, AddressPool, NetMask};
use mm1::ask::proto::Request;
use mm1::ask::{Ask, AskErrorKind, Reply};
use mm1::common::error::HasErrorKind;
use mm1::core::context::Fork;
use mm1::core::envelope::dispatch;
use mm1::proto::message;
use mm1::test::rt::event::EventResolveResult;
use mm1::test::rt::{MainActorOutcome, TestRuntime, query};
use tokio::time;

#[tokio::test]
async fn ergonomics() {
    time::pause();

    let rt = TestRuntime::<()>::new();
    let subnet = AddressPool::new("<ff:>/16".parse().unwrap());
    let server_lease = subnet.lease(NetMask::M_32).unwrap();
    let client_lease = subnet.lease(NetMask::M_32).unwrap();

    let server_address = server_lease.address;
    let client_address = client_lease.address;

    rt.add_actor(server_address, Some(server_lease), server)
        .await
        .unwrap();
    let server_recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(server_recv.task_key.actor, server_address);

    rt.add_actor(
        client_address,
        Some(client_lease),
        (client, (server_address,)),
    )
    .await
    .unwrap();

    let mut client_tells = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(client_tells.task_key.actor, client_address);
    let envelope = client_tells.take_envelope();
    assert_eq!(envelope.header().to, server_address);

    client_tells.resolve_ok(());

    let client_recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(client_recv.task_key.actor, client_address);

    server_recv.resolve_ok(envelope);

    let mut server_tells = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(server_tells.task_key.actor, server_address);
    let envelope = server_tells.take_envelope();
    assert_eq!(envelope.header().to, client_address);

    client_recv.resolve_ok(envelope);

    let mut client_tells = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(client_tells.task_key.actor, client_address);
    let envelope = client_tells.take_envelope();
    assert_eq!(envelope.header().to, server_address);

    client_tells.resolve_ok(());

    let client_recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(client_recv.task_key.actor, client_address);

    time::sleep(Duration::from_millis(200)).await;

    let client_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(client_done.address, client_address);
}

#[message]
struct Rq;

#[derive(Debug)]
#[message]
struct Rs;

async fn client<Ctx>(ctx: &mut Ctx, server_address: Address)
where
    Ctx: Ask + Fork,
{
    let Rs = ctx
        .ask(server_address, Rq, Duration::from_millis(100))
        .await
        .unwrap();
    let e = ctx
        .ask::<Rq, Rs>(server_address, Rq, Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(e.kind(), AskErrorKind::Timeout);
}

async fn server<Ctx>(ctx: &mut Ctx)
where
    Ctx: Reply,
{
    while let Ok(envelope) = ctx.recv().await {
        let (header, Rq { .. }) = dispatch!(match envelope {
            Request::<Rq> { header, payload } => (header, payload),
        });
        let () = ctx.reply(header, Rs).await.unwrap();
    }
}
