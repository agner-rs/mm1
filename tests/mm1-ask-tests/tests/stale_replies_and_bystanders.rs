//! Repro for #136: `mm1-ask` ignores the response id, so it accepts stale
//! replies and destroys unrelated (bystander) messages that arrive during an
//! ask.
//!
//! Both tests drive the event-stepped `mm1-test-rt`, so the message timing is
//! fully deterministic. They assert on the syscall events the client makes: a
//! correct ask that meets a stale reply or a bystander keeps waiting (it recvs
//! again), while today's ask returns or fails on the first message — so the
//! extra `recv` (and, for the bystander, the re-injecting `forward`) never
//! happens and the `convert` fails.

use std::time::Duration;

use mm1::address::{Address, AddressPool, NetMask};
use mm1::ask::Ask;
use mm1::ask::proto::{Request, RequestHeader, Response, ResponseHeader};
use mm1::core::envelope::{Envelope, EnvelopeHeader};
use mm1::proto::message;
use mm1::test::rt::event::EventResolveResult;
use mm1::test::rt::{MainActorOutcome, TestRuntime, query};
use tokio::time;

#[message]
struct Rq;

#[derive(Debug)]
#[message]
struct Rs;

#[derive(Debug)]
#[message]
struct Hello;

/// A stale reply is one whose id does not match the pending request; the
/// correct behaviour is to discard it and keep waiting, never to return it.
#[tokio::test]
async fn stale_reply_is_not_accepted() {
    time::pause();

    let rt = TestRuntime::<()>::new();
    let subnet = AddressPool::new("<ff:>/16".parse().unwrap());
    let server_lease = subnet.lease(NetMask::M_32).unwrap();
    let client_lease = subnet.lease(NetMask::M_32).unwrap();
    let server_address = server_lease.address;
    let client_address = client_lease.address;

    rt.add_actor(
        client_address,
        Some(client_lease),
        (client_two_asks, (server_address,)),
    )
    .await
    .unwrap();

    // ask #1: capture its request id, then let it time out (leave the recv
    // pending and advance virtual time past the 100ms deadline).
    let mut tell1 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    let (req1_id, reply_to) = take_request_id(tell1.take_envelope());
    assert_eq!(reply_to, client_address);
    tell1.resolve_ok(());

    let _recv1 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    time::sleep(Duration::from_millis(200)).await;

    // ask #2: a fresh request with a different id.
    let mut tell2 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    let (req2_id, _) = take_request_id(tell2.take_envelope());
    assert_ne!(req1_id, req2_id, "each ask must use a fresh request id");
    tell2.resolve_ok(());

    // Feed ask #2 the *stale* reply for ask #1 (id = req1_id).
    let recv2 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    recv2.resolve_ok(response_envelope(client_address, req1_id));

    // A correct ask discards the stale reply and recvs again; a buggy ask
    // returns the stale value and the client finishes here, so this is not a
    // recv.
    let _recv3 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .expect("#136: stale reply was accepted (ask returned instead of waiting)");

    // Drain: let ask #2 time out and the actor finish.
    time::sleep(Duration::from_millis(200)).await;
    expect_actor_quit_ok(&rt, client_address).await;
    let done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(done.address, client_address);
    done.remove_actor_entry().await.unwrap();
}

/// A message that is not the response must not be destroyed by an in-flight
/// ask; it must be put back so the actor's normal receive loop can get it.
#[tokio::test]
async fn bystander_is_not_destroyed() {
    time::pause();

    let rt = TestRuntime::<()>::new();
    let subnet = AddressPool::new("<ff:>/16".parse().unwrap());
    let server_lease = subnet.lease(NetMask::M_32).unwrap();
    let client_lease = subnet.lease(NetMask::M_32).unwrap();
    let server_address = server_lease.address;
    let client_address = client_lease.address;

    rt.add_actor(
        client_address,
        Some(client_lease),
        (client_one_ask, (server_address,)),
    )
    .await
    .unwrap();

    let mut tell = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    let (req_id, reply_to) = take_request_id(tell.take_envelope());
    assert_eq!(reply_to, client_address);
    tell.resolve_ok(());

    // Deliver an unrelated message during the ask.
    let recv1 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    recv1.resolve_ok(hello_envelope(client_address));

    // A correct ask keeps the bystander and recvs again; a buggy ask fails on
    // the non-matching message and the client finishes, so this is not a recv.
    let recv2 = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .expect("#136: bystander destroyed the ask (ask returned instead of waiting)");
    recv2.resolve_ok(response_envelope(client_address, req_id));

    // Having answered the ask, the client must put the bystander back — it
    // forwards it to its own address.
    let mut reinject = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .expect("#136: bystander was not re-injected after the ask");
    assert_eq!(
        reinject.to, client_address,
        "bystander re-injected to the wrong address"
    );
    assert!(
        reinject.take_envelope().is::<Hello>(),
        "re-injected message is not the bystander"
    );
    reinject.resolve_ok(());

    expect_actor_quit_ok(&rt, client_address).await;
    let done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(done.address, client_address);
    done.remove_actor_entry().await.unwrap();
}

async fn expect_actor_quit_ok(rt: &TestRuntime<()>, address: Address) {
    let quit = rt.expect_next_event().await.expect::<query::Quit>();
    assert_eq!(quit.task_key.actor, address);
    assert_eq!(quit.task_key.context, address);
    assert!(quit.result.is_ok());
    quit.stop_tasks().await.unwrap();
}

async fn client_two_asks<Ctx>(ctx: &mut Ctx, server: Address)
where
    Ctx: Ask,
{
    let _ = ctx
        .ask_nofork::<Rq, Rs>(server, Rq, Duration::from_millis(100))
        .await;
    let _ = ctx
        .ask_nofork::<Rq, Rs>(server, Rq, Duration::from_millis(100))
        .await;
}

async fn client_one_ask<Ctx>(ctx: &mut Ctx, server: Address)
where
    Ctx: Ask,
{
    let _ = ctx
        .ask_nofork::<Rq, Rs>(server, Rq, Duration::from_millis(100))
        .await;
}

fn take_request_id(envelope: Envelope) -> (u64, Address) {
    let request = envelope
        .cast::<Request<Rq>>()
        .unwrap_or_else(|_| panic!("expected a Request<Rq>"));
    let (
        Request {
            header: RequestHeader { id, reply_to },
            ..
        },
        _empty,
    ) = request.take();
    (id, reply_to)
}

fn response_envelope(to: Address, id: u64) -> Envelope {
    Envelope::new(
        EnvelopeHeader::to_address(to),
        Response {
            header:  ResponseHeader { id },
            payload: Rs,
        },
    )
    .into_erased()
}

fn hello_envelope(to: Address) -> Envelope {
    Envelope::new(EnvelopeHeader::to_address(to), Hello).into_erased()
}
