use std::pin::pin;
use std::time::Duration;

use futures::FutureExt;
use mm1::address::{Address, AddressPool, NetAddress, NetMask};
use mm1::common::error::ErrorOf;
use mm1::common::log;
use mm1::core::context::{
    Bind, BindArgs, Fork, InitDone, Linking, Messaging, Quit, RecvErrorKind, Start, Stop, Tell,
    Watching,
};
use mm1::core::envelope::{Envelope, EnvelopeHeader};
use mm1::proto::message;
use mm1::proto::system::WatchRef;
use mm1::test::rt::event::{EventResolve, EventResolveResult};
use mm1::test::rt::{ForkTaskOutcome, MainActorOutcome, TaskKey, TestRuntime, query};
use tokio::time;

#[derive(Debug)]
struct Runnable {
    name: &'static str,
}

#[tokio::test]
async fn t_simplest() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    rt.add_actor(lease_a.address, Some(lease_a), a)
        .await
        .unwrap();

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(_ctx: &mut C) {}
}

#[tokio::test]
async fn t_spawn() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), (a, ("hello-spawn",)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Spawn<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let (runnable, query_spawn) = query.take_runnable();
    assert_eq!(runnable.name, "hello-spawn");
    query_spawn.resolve_ok(lease_b.address);

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C, task_name: &'static str)
    where
        C: Start<Runnable>,
    {
        let addr = ctx.spawn(Runnable { name: task_name }, true).await.unwrap();
        log::info!("spawned {}", addr);
    }
}

#[tokio::test]
async fn t_start() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), (a, ("hello-start",)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Start<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let (runnable, query_start) = query.take_runnable();
    assert_eq!(runnable.name, "hello-start");
    query_start.resolve_ok(lease_b.address);

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C, task_name: &'static str)
    where
        C: Start<Runnable>,
    {
        let addr = ctx
            .start(
                Runnable { name: task_name },
                true,
                Duration::from_millis(100),
            )
            .await
            .unwrap();
        log::info!("started {}", addr);
    }
}

#[tokio::test]
async fn t_recv() {
    #[message]
    struct Hello;

    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let envelope = Envelope::new(EnvelopeHeader::to_address(address_a), Hello).into_erased();
    query.resolve_ok(envelope);

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C)
    where
        C: Messaging,
    {
        let envelope = ctx.recv().await.unwrap();
        log::info!("received {:?}", envelope);
    }
}

#[tokio::test]
async fn t_init_done() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::InitDone>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.address, address_a);
    query.resolve(());

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C)
    where
        C: InitDone + Messaging,
    {
        ctx.init_done(ctx.address()).await
    }
}

#[tokio::test]
async fn t_recv_close() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::RecvClose>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    query.resolve(());

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C)
    where
        C: Messaging,
    {
        let envelope = ctx.close().await;
        log::info!("received {:?}", envelope);
    }
}

#[tokio::test]
async fn t_fork_and_run() {
    #[message]
    struct Hello;

    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_60).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Fork<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let fork_context = rt.new_context(TaskKey::fork(address_a, address_b), Some(lease_b));
    query.resolve_ok(fork_context);

    let query_fork_run = rt
        .expect_next_event()
        .await
        .convert::<query::ForkRun>()
        .unwrap();

    let pending_task = query_fork_run.resolve();

    let query_main_recv = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();

    pending_task.run().await.unwrap();

    let query_fork_recv = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();

    let reply_to = query_fork_recv.task_key.context;
    query_fork_recv
        .resolve_ok(Envelope::new(EnvelopeHeader::to_address(reply_to), Hello).into_erased());

    let task_done = rt
        .expect_next_event()
        .await
        .convert::<ForkTaskOutcome>()
        .unwrap();
    assert_eq!(task_done.task_key, TaskKey::fork(address_a, address_b));

    let reply_to = query_main_recv.task_key.context;
    query_main_recv
        .resolve_ok(Envelope::new(EnvelopeHeader::to_address(reply_to), Hello).into_erased());

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C)
    where
        C: Fork + Messaging,
    {
        let fork = ctx.fork().await.unwrap();
        log::info!("forked {:?}", fork.address());
        fork.run(|mut ctx| {
            async move {
                log::info!("hey! I'm forked [fork-addr: {}]", ctx.address());
                let envelope = ctx.recv().await.unwrap();
                log::info!("[forked] received: {:?}", envelope);
            }
        })
        .await;

        let envelope = ctx.recv().await.unwrap();
        log::info!("[main] received: {:?}", envelope);
    }
}

#[tokio::test]
async fn t_bind_net_address() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let address_a = lease_a.address;
    let rt = TestRuntime::<Runnable>::new();

    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let bind = dbg!(rt.expect_next_event().await.expect::<query::Bind<_>>());
    bind.resolve_ok(());

    let recv = dbg!(rt.expect_next_event().await.expect::<query::Recv>());
    recv.resolve_err(ErrorOf::new(RecvErrorKind::Closed, "closed"));

    let _done = dbg!(rt.expect_next_event().await.expect::<MainActorOutcome>());

    async fn a<C>(ctx: &mut C)
    where
        C: Bind<NetAddress> + Messaging,
    {
        ctx.bind(BindArgs {
            bind_to:    "<aa:>/32".parse().unwrap(),
            inbox_size: 1024,
        })
        .await
        .expect("bind failure");

        let _ = ctx.recv().await.expect_err("should have been closed");
    }
}

#[tokio::test]
async fn t_tell() {
    #[message]
    struct Hello;

    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.envelope.header().to, address_b);
    assert!(query.envelope.is::<Hello>());

    query.resolve_ok(());

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C, to: Address)
    where
        C: Messaging,
    {
        log::info!("sending to {}", to);
        ctx.tell(to, Hello).await.unwrap();
    }
}

#[tokio::test]
async fn t_quit() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Quit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    query.stop_tasks().await.unwrap();

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();
    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C)
    where
        C: Quit,
    {
        ctx.quit_ok().await;
    }
}

#[tokio::test]
async fn t_watching() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Watch>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);

    let w = WatchRef::from_u64(123);
    query.resolve(w);

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Unwatch>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.watch_ref, w);

    query.resolve(());

    let main_actor_done = rt
        .expect_next_event()
        .await
        .convert::<MainActorOutcome>()
        .unwrap();

    assert_eq!(main_actor_done.kind.address, address_a);
    main_actor_done.remove_actor_entry().await.unwrap();

    assert!(matches!(
        pin!(rt.next_event()).now_or_never().unwrap(),
        Ok(None)
    ));

    async fn a<C>(ctx: &mut C, peer: Address)
    where
        C: Watching,
    {
        let w = ctx.watch(peer).await;
        log::info!("watching {} => {}", peer, w);
        ctx.unwatch(w).await;
    }
}

#[tokio::test]
async fn t_linking() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::SetTrapExit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert!(query.enable);
    query.resolve(());

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Link>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(());

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Unlink>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(());

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::SetTrapExit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert!(!query.enable);
    query.resolve(());

    async fn a<C>(ctx: &mut C, peer: Address)
    where
        C: Linking,
    {
        ctx.set_trap_exit(true).await;
        ctx.link(peer).await;
        ctx.unlink(peer).await;
        ctx.set_trap_exit(false).await;
    }
}

#[tokio::test]
async fn t_stop() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = TestRuntime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Exit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(true);

    let query = rt
        .expect_next_event()
        .await
        .convert::<query::Kill>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(true);

    async fn a<C>(ctx: &mut C, peer: Address)
    where
        C: Stop,
    {
        ctx.exit(peer).await;
        ctx.kill(peer).await;
    }
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            "mm1_test_rt::*=TRACE".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
