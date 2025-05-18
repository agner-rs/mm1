use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mm1_address::address::Address;
use mm1_address::pool::Pool as SubnetPool;
use mm1_address::subnet::NetMask;
use mm1_common::log::{debug, info};
use mm1_common::types::Never;
use mm1_core::context::{Ask, Fork, InitDone, Quit, Recv, Start, Tell};
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_node::runtime::{Local, Rt};
use mm1_proto::message;
use mm1_proto_sup::uniform;
use mm1_sup::common::child_spec::{ChildSpec, ChildType, InitType};
use mm1_sup::common::factory::{ActorFactory, ActorFactoryMut};
use mm1_sup::uniform::{UniformSup, uniform_sup};
use mm1_test_rt::query;
use mm1_test_rt::runtime::TestRuntime;
use tokio::time;

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "*=TRACE".parse().unwrap(),
        ],
    }
}

#[test]
fn test_01() {
    let _ = mm1_logger::init(&logger_config());

    #[derive(Debug)]
    #[message]
    struct Hi {
        worker_id:      usize,
        worker_address: Address,
    }

    async fn worker<Ctx>(ctx: &mut Ctx, reply_to: Address, delay: Duration) -> Never
    where
        Ctx: Recv + Tell + Quit + InitDone,
    {
        static WORKER_ID: AtomicUsize = AtomicUsize::new(0);
        let worker_id = WORKER_ID.fetch_add(1, Ordering::Relaxed);
        debug!(
            "worker[{}] started, replying to {} in {:?}",
            worker_id, reply_to, delay
        );
        ctx.init_done(ctx.address()).await;

        tokio::time::sleep(delay).await;
        let () = ctx
            .tell(
                reply_to,
                Hi {
                    worker_id,
                    worker_address: ctx.address(),
                },
            )
            .await
            .expect("tell");

        std::future::pending().await
    }

    async fn main<Ctx>(ctx: &mut Ctx)
    where
        Ctx: Fork + Recv + Tell + Start<Local>,
    {
        let factory = ActorFactoryMut::new(|(reply_to, duration): (Address, Duration)| {
            Local::actor((worker, (reply_to, duration)))
        });
        let child = ChildSpec {
            launcher:     factory,
            child_type:   ChildType::Temporary,
            init_type:    InitType::WithAck {
                start_timeout: Duration::from_secs(1),
            },
            stop_timeout: Duration::from_secs(1),
        };
        let sup = UniformSup::new(child);
        let sup_runnable = Local::actor((uniform_sup, (sup,)));

        let sup_addr = ctx
            .start(sup_runnable, true, Duration::from_secs(1))
            .await
            .expect("failed to run `sup_runnable`");
        let main_addr = ctx.address();

        info!("MAIN: {}", main_addr);
        info!("SUP:  {}", sup_addr);

        let mut workers = HashSet::new();
        for i in 0..128 {
            let (start_response, _) = ctx
                .ask(sup_addr, |reply_to| {
                    uniform::StartRequest {
                        reply_to,
                        args: (main_addr, Duration::from_millis(i % 1024)),
                    }
                })
                .await
                .expect("ask")
                .cast::<uniform::StartResponse>()
                .expect("cast")
                .take();
            let started = start_response.expect("start-response");
            debug!("started[{}]: {}", i, started);

            assert!(workers.insert(started), "duplicate address: {}", started);
        }

        info!("WORKERS: {}", workers.len());

        while !workers.is_empty() {
            let (
                Hi {
                    worker_id,
                    worker_address,
                },
                _,
            ) = ctx.recv().await.expect("recv").cast().expect("cast").take();

            assert!(
                workers.remove(&worker_address),
                "unknown address: {}",
                worker_address
            );
            debug!("WORKER[{}]@{} said Hi", worker_id, worker_address);

            let (stop_response, _) = ctx
                .ask(sup_addr, |reply_to| {
                    uniform::StopRequest {
                        reply_to,
                        child: worker_address,
                    }
                })
                .await
                .expect("ask")
                .cast::<uniform::StopResponse>()
                .expect("cast")
                .take();

            let () = stop_response.expect("stop-response");

            debug!("WORKER[{}]@{} terminated", worker_id, worker_address);
        }
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(Local::actor(main))
        .expect("Rt::run");
}

#[tokio::test]
async fn test_02() {
    let _ = mm1_logger::init(&logger_config());

    time::pause();

    #[derive(Debug, Clone, Copy)]
    struct Runnable<A> {
        seq_no: usize,
        args:   A,
    }

    #[derive(Debug, Default)]
    struct Factory {
        seq_no: AtomicUsize,
    }

    impl ActorFactory for Factory {
        type Args = &'static str;
        type Runnable = Runnable<Self::Args>;

        fn produce(&self, args: Self::Args) -> Self::Runnable {
            let seq_no = self.seq_no.fetch_add(1, Ordering::Relaxed);

            Runnable { seq_no, args }
        }
    }

    let node_subnet = SubnetPool::new("<ff:>/32".parse().unwrap());
    let sup_lease = node_subnet.lease(NetMask::M_64).unwrap();

    eprintln!("SUP: {}", sup_lease.address);

    let child_spec = ChildSpec {
        launcher:     Factory::default(),
        child_type:   ChildType::Temporary,
        init_type:    InitType::NoAck,
        stop_timeout: Duration::from_secs(1),
    };
    let uniform_sup = UniformSup::new(child_spec);
    let mut rt = TestRuntime::<Runnable<&'static str>>::new()
        .with_actor(
            sup_lease.address,
            (mm1_sup::uniform::uniform_sup, (uniform_sup,)),
        )
        .unwrap();

    let (addr, set_trap_exit) = rt
        .next_event()
        .await
        .unwrap()
        .expect_query::<query::SetTrapExit>();
    assert_eq!(addr, sup_lease.address);
    assert!(set_trap_exit.enable);
    let _ = set_trap_exit.outcome_tx.send(());

    let (addr, init_done) = rt
        .next_event()
        .await
        .unwrap()
        .expect_query::<query::InitDone>();
    assert_eq!(addr, sup_lease.address);
    assert_eq!(init_done.address, sup_lease.address);
    let _ = init_done.outcome_tx.send(());

    let event = rt.next_event().await.unwrap();
    let (addr, recv) = event.expect_query::<query::Recv>();
    assert_eq!(addr, sup_lease.address);

    let client_lease = node_subnet.lease(NetMask::M_64).unwrap();

    let envelope = Envelope::new(
        EnvelopeInfo::new(addr),
        mm1_proto_sup::uniform::StartRequest {
            reply_to: client_lease.address,
            args:     "hello!",
        },
    )
    .into_erased();
    let _ = recv.outcome_tx.send(Ok(envelope));

    let (addr, fork) = rt
        .next_event()
        .await
        .unwrap()
        .expect_query::<query::Fork<_>>();
    assert_eq!(addr, sup_lease.address);
    let fork_lease = node_subnet.lease(NetMask::M_64).unwrap();
    let _ = fork.outcome_tx.send(Ok(
        rt.new_fork_context(sup_lease.address, fork_lease.address)
    ));

    let (addr, fork_run) = rt.next_event().await.unwrap().expect_query::<query::Run>();
    assert_eq!(addr, sup_lease.address);
    rt.add_fork_task(sup_lease.address, fork_run.task).unwrap();

    let (addr, spawn) = rt
        .next_event()
        .await
        .unwrap()
        .expect_query::<query::Spawn<_>>();
    let _ = spawn.runnable.seq_no;
    assert_eq!(spawn.runnable.args, "hello!");

    assert_eq!(addr, sup_lease.address);
    let spawn_lease = node_subnet.lease(NetMask::M_64).unwrap();
    let _ = spawn.outcome_tx.send(Ok(spawn_lease.address));

    let (addr, tell) = rt.next_event().await.unwrap().expect_query::<query::Tell>();
    assert_eq!(addr, sup_lease.address);
    assert_eq!(tell.envelope.info().to, sup_lease.address);
    let _ = tell.outcome_tx.send(Ok(()));

    let (addr, tell) = rt.next_event().await.unwrap().expect_query::<query::Tell>();
    assert_eq!(addr, sup_lease.address);
    assert_eq!(tell.envelope.info().to, client_lease.address);
    let (response, _) = tell
        .envelope
        .cast::<mm1_proto_sup::uniform::StartResponse>()
        .unwrap()
        .take();
    assert_eq!(response.unwrap(), spawn_lease.address);
}
