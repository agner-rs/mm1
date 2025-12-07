use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mm1::address::{Address, AddressPool, NetAddress, NetMask};
use mm1::ask::Ask;
use mm1::common::Never;
use mm1::common::log::{debug, info};
use mm1::core::context::{Fork, InitDone, Messaging, Quit, Start, Tell};
use mm1::core::envelope::{Envelope, EnvelopeHeader};
use mm1::proto::message;
use mm1::proto::sup::uniform;
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};
use mm1::sup::common::{ActorFactory, ActorFactoryMut, ChildSpec, InitType};
use mm1::sup::uniform::{UniformSup, child_type, uniform_sup};
use mm1::test::rt::event::EventResolve;
use mm1::test::rt::{TaskKey, query};
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
        Ctx: Messaging + Quit + InitDone,
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
        Ctx: Fork + Messaging + Start<Local> + Ask,
    {
        let factory = ActorFactoryMut::new(|(reply_to, duration): (Address, Duration)| {
            local::boxed_from_fn((worker, (reply_to, duration)))
        });
        let child = ChildSpec::new(factory)
            .with_child_type(child_type::Temporary)
            .with_init_type(InitType::WithAck {
                start_timeout: Duration::from_secs(1),
            })
            .with_stop_timeout(Duration::from_secs(1));
        let sup = UniformSup::new(child);
        let sup_runnable = local::boxed_from_fn((uniform_sup, (sup,)));

        let sup_addr = ctx
            .start(sup_runnable, true, Duration::from_secs(1))
            .await
            .expect("failed to run `sup_runnable`");
        let main_addr = ctx.address();

        info!("MAIN: {}", main_addr);
        info!("SUP:  {}", sup_addr);

        let mut workers = HashSet::new();
        for i in 0..128 {
            let start_response: uniform::StartResponse = ctx
                .ask(
                    sup_addr,
                    uniform::StartRequest {
                        args: (main_addr, Duration::from_millis(i % 1024)),
                    },
                    Duration::from_millis(100),
                )
                .await
                .expect("ask");
            let started = start_response.expect("start-response");
            debug!("started[{}]: {}", i, started);

            assert!(workers.insert(started), "duplicate address: {started}");
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
                "unknown address: {worker_address}"
            );
            debug!("WORKER[{}]@{} said Hi", worker_id, worker_address);

            let stop_response: uniform::StopResponse = ctx
                .ask(
                    sup_addr,
                    uniform::StopRequest {
                        child: worker_address,
                    },
                    Duration::from_millis(100),
                )
                .await
                .expect("ask");

            let () = stop_response.expect("stop-response");

            debug!("WORKER[{}]@{} terminated", worker_id, worker_address);
        }
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(local::boxed_from_fn(main))
        .expect("Rt::run");
}

#[tokio::test]
async fn test_02() {
    let _ = mm1_logger::init(&logger_config());

    time::pause();

    #[derive(Debug, Clone, Copy)]
    struct Runnable<A> {
        #[allow(dead_code)]
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

    let node_subnet = AddressPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_subnet.lease(NetMask::M_56).unwrap();
    let subnet_sup = AddressPool::new(NetAddress {
        address: lease_a.address,
        mask:    lease_a.mask,
    });
    let lease_sup = subnet_sup.lease(NetMask::M_64).unwrap();
    let address_sup = lease_sup.address;

    info!("SUP: {}", lease_a.address);

    let child_spec = ChildSpec::new(Factory::default())
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::NoAck)
        .with_stop_timeout(Duration::from_secs(1));
    let uniform_sup = UniformSup::new(child_spec);
    let rt = mm1::test::rt::TestRuntime::<Runnable<&'static str>>::new()
        .with_actor(
            address_sup,
            Some(lease_a),
            (mm1::sup::uniform::uniform_sup, (uniform_sup,)),
        )
        .await
        .unwrap();

    let set_trap_exit = rt
        .expect_next_event()
        .await
        .convert::<query::SetTrapExit>()
        .unwrap();
    assert_eq!(set_trap_exit.task_key.actor, address_sup);
    assert!(set_trap_exit.enable);
    set_trap_exit.resolve(());

    let init_done = rt
        .expect_next_event()
        .await
        .convert::<query::InitDone>()
        .unwrap();
    assert_eq!(init_done.task_key.actor, address_sup);
    assert_eq!(init_done.address, address_sup);
    init_done.resolve(());

    let recv = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(recv.task_key.actor, address_sup);
    let lease_client = node_subnet.lease(NetMask::M_64).unwrap();
    let address_client = lease_client.address;
    let envelope = Envelope::new(
        EnvelopeHeader::to_address(address_sup),
        mm1::ask::proto::Request {
            header:  mm1::ask::proto::RequestHeader {
                id:       Default::default(),
                reply_to: address_client,
            },
            payload: uniform::StartRequest { args: "hello!" },
        },
    )
    .into_erased();
    recv.resolve(Ok(envelope));

    let fork = rt
        .expect_next_event()
        .await
        .convert::<query::Fork<_>>()
        .unwrap();
    assert_eq!(fork.task_key.actor, address_sup);
    let lease_fork = subnet_sup.lease(NetMask::M_60).unwrap();
    let address_fork = lease_fork.address;
    let fork_context = rt.new_context(TaskKey::fork(address_sup, address_fork), Some(lease_fork));
    fork.resolve(Ok(fork_context));

    let fork_run = rt
        .expect_next_event()
        .await
        .convert::<query::ForkRun>()
        .unwrap();
    assert_eq!(fork_run.task_key.actor, address_sup);
    assert_eq!(fork_run.task_key.context, address_fork);
    let fork_pending = fork_run.resolve();

    let recv = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(recv.task_key.actor, address_sup);
    assert_eq!(recv.task_key.context, address_sup);

    fork_pending.run().await.unwrap();

    let spawn = rt
        .expect_next_event()
        .await
        .convert::<query::Spawn<_>>()
        .unwrap();
    assert_eq!(spawn.task_key.actor, address_sup);
    assert_eq!(spawn.task_key.context, address_fork);
    let (runnable, spawn) = spawn.take_runnable();
    assert_eq!(runnable.args, "hello!");

    let lease_child = node_subnet.lease(NetMask::M_50).unwrap();
    let address_child = lease_child.address;
    spawn.resolve(Ok(address_child));

    let mut tell = rt
        .expect_next_event()
        .await
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(tell.task_key.actor, address_sup);
    assert_eq!(tell.task_key.context, address_fork);
    assert_eq!(tell.envelope.header().to, address_sup);
    let envelope = tell.take_envelope();

    recv.resolve(Ok(envelope));

    let link = rt
        .expect_next_event()
        .await
        .convert::<query::Link>()
        .unwrap();
    assert_eq!(link.task_key.actor, address_sup);
    assert_eq!(link.task_key.context, address_sup);
    assert_eq!(link.peer, address_child);
    link.resolve(());

    let _recv = rt
        .expect_next_event()
        .await
        .convert::<query::Recv>()
        .unwrap();

    tell.resolve(Ok(()));

    let mut tell = rt
        .expect_next_event()
        .await
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(tell.task_key.actor, address_sup);
    assert_eq!(tell.task_key.context, address_fork);
    let envelope = tell.take_envelope();
    assert_eq!(envelope.header().to, address_client);
    let (response, _envelope) = envelope
        .cast::<mm1::ask::proto::Response<uniform::StartResponse>>()
        .unwrap()
        .take();
    assert_eq!(response.payload.unwrap(), address_child);
}
