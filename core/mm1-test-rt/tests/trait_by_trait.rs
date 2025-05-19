use std::pin::pin;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::pool::Pool as SubnetPool;
use mm1_address::subnet::NetMask;
use mm1_common::log;
use mm1_core::context::{Fork, InitDone, Linking, Quit, Recv, Start, Stop, Tell, Watching};
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_core::prim::message;
use mm1_proto_system::WatchRef;
use mm1_test_rt::rt::event::{EventResolve, EventResolveResult};
use mm1_test_rt::rt::{ForkTaskOutcome, MainActorOutcome, Runtime, TaskKey, query};
use tokio::time;

#[derive(Debug)]
struct Runnable {
    name: &'static str,
}

#[tokio::test]
async fn t_simplest() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    rt.add_actor(lease_a.address, Some(lease_a), a)
        .await
        .unwrap();

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), (a, ("hello-spawn",)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Spawn<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let (runnable, query_spawn) = query.take_runnable();
    assert_eq!(runnable.name, "hello-spawn");
    query_spawn.resolve_ok(lease_b.address);

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), (a, ("hello-start",)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Start<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let (runnable, query_start) = query.take_runnable();
    assert_eq!(runnable.name, "hello-start");
    query_start.resolve_ok(lease_b.address);

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let envelope = Envelope::new(EnvelopeInfo::new(address_a), Hello).into_erased();
    query.resolve_ok(envelope);

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        C: Recv,
    {
        let envelope = ctx.recv().await.unwrap();
        log::info!("received {:?}", envelope);
    }
}

#[tokio::test]
async fn t_init_done() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::InitDone>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.address, address_a);
    query.resolve(());

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        C: InitDone + Recv,
    {
        ctx.init_done(ctx.address()).await
    }
}

#[tokio::test]
async fn t_recv_close() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::RecvClose>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    query.resolve(());

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        C: Recv,
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_60).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Fork<_>>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    let fork_context = rt.new_context(TaskKey::fork(address_a, address_b), Some(lease_b));
    query.resolve_ok(fork_context);

    let query_fork_run = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::ForkRun>()
        .unwrap();

    let pending_task = query_fork_run.resolve().await;

    let query_main_recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();

    pending_task.run().await.unwrap();

    let query_fork_recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();

    let reply_to = query_fork_recv.task_key.context;
    query_fork_recv.resolve_ok(Envelope::new(EnvelopeInfo::new(reply_to), Hello).into_erased());

    let task_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<ForkTaskOutcome>()
        .unwrap();
    assert_eq!(task_done.task_key, TaskKey::fork(address_a, address_b));

    let reply_to = query_main_recv.task_key.context;
    query_main_recv.resolve_ok(Envelope::new(EnvelopeInfo::new(reply_to), Hello).into_erased());

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        C: Fork + Recv,
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
async fn t_tell() {
    #[message]
    struct Hello;

    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.envelope.info().to, address_b);
    assert!(query.envelope.is::<Hello>());

    query.resolve_ok(());

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        C: Tell,
    {
        log::info!("sending to {}", to);
        ctx.tell(to, Hello).await.unwrap();
    }
}

#[tokio::test]
async fn t_quit() {
    let _ = mm1_logger::init(&logger_config());
    time::pause();

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    rt.add_actor(address_a, Some(lease_a), a).await.unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Quit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);

    query.stop_tasks().await.unwrap();

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Watch>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);

    let w = WatchRef::from_u64(123);
    query.resolve(w);

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Unwatch>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.watch_ref, w);

    query.resolve(());

    let main_actor_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::SetTrapExit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert!(query.enable);
    query.resolve(());

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Link>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(());

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Unlink>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(());

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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

    let node_net = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_net.lease(NetMask::M_48).unwrap();
    let lease_b = node_net.lease(NetMask::M_64).unwrap();
    let rt = Runtime::<Runnable>::new();
    let address_a = lease_a.address;
    let address_b = lease_b.address;
    rt.add_actor(address_a, Some(lease_a), (a, (address_b,)))
        .await
        .unwrap();

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Exit>()
        .unwrap();
    assert_eq!(query.task_key.actor, address_a);
    assert_eq!(query.task_key.context, address_a);
    assert_eq!(query.peer, address_b);
    query.resolve(true);

    let query = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
