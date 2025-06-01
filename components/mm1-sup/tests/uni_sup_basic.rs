use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mm1_address::address::Address;
use mm1_address::pool::Pool as SubnetPool;
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_ask::Ask;
use mm1_common::log::{debug, info};
use mm1_common::types::Never;
use mm1_core::context::{Fork, InitDone, Messaging, Quit, Start, Tell};
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_node::runtime::{Local, Rt};
use mm1_proto::message;
use mm1_proto_sup::uniform;
use mm1_sup::common::child_spec::{ChildSpec, ChildType, InitType};
use mm1_sup::common::factory::{ActorFactory, ActorFactoryMut};
use mm1_sup::uniform::{UniformSup, uniform_sup};
use mm1_test_rt::rt::event::EventResolve;
use mm1_test_rt::rt::{TaskKey, query};
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
        .run(Local::actor(main))
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

    let node_subnet = SubnetPool::new("<ff:>/32".parse().unwrap());
    let lease_a = node_subnet.lease(NetMask::M_56).unwrap();
    let subnet_sup = SubnetPool::new(NetAddress {
        address: lease_a.address,
        mask:    lease_a.mask,
    });
    let lease_sup = subnet_sup.lease(NetMask::M_64).unwrap();
    let address_sup = lease_sup.address;

    info!("SUP: {}", lease_a.address);

    let child_spec = ChildSpec {
        launcher:     Factory::default(),
        child_type:   ChildType::Temporary,
        init_type:    InitType::NoAck,
        stop_timeout: Duration::from_secs(1),
    };
    let uniform_sup = UniformSup::new(child_spec);
    let rt = mm1_test_rt::rt::Runtime::<Runnable<&'static str>>::new()
        .with_actor(
            address_sup,
            Some(lease_a),
            (mm1_sup::uniform::uniform_sup, (uniform_sup,)),
        )
        .await
        .unwrap();

    let set_trap_exit = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::SetTrapExit>()
        .unwrap();
    assert_eq!(set_trap_exit.task_key.actor, address_sup);
    assert!(set_trap_exit.enable);
    set_trap_exit.resolve(());

    let init_done = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::InitDone>()
        .unwrap();
    assert_eq!(init_done.task_key.actor, address_sup);
    assert_eq!(init_done.address, address_sup);
    init_done.resolve(());

    let recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(recv.task_key.actor, address_sup);
    let lease_client = node_subnet.lease(NetMask::M_64).unwrap();
    let address_client = lease_client.address;
    let envelope = Envelope::new(
        EnvelopeHeader::to_address(address_sup),
        mm1_proto_ask::Request {
            header:  mm1_proto_ask::RequestHeader {
                id:       (),
                reply_to: address_client,
            },
            payload: mm1_proto_sup::uniform::StartRequest { args: "hello!" },
        },
    )
    .into_erased();
    recv.resolve(Ok(envelope));

    let fork = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Fork<_>>()
        .unwrap();
    assert_eq!(fork.task_key.actor, address_sup);
    let lease_fork = subnet_sup.lease(NetMask::M_60).unwrap();
    let address_fork = lease_fork.address;
    let fork_context = rt.new_context(TaskKey::fork(address_sup, address_fork), Some(lease_fork));
    fork.resolve(Ok(fork_context));

    let fork_run = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::ForkRun>()
        .unwrap();
    assert_eq!(fork_run.task_key.actor, address_sup);
    assert_eq!(fork_run.task_key.context, address_fork);
    let fork_pending = fork_run.resolve().await;

    let recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();
    assert_eq!(recv.task_key.actor, address_sup);
    assert_eq!(recv.task_key.context, address_sup);

    fork_pending.run().await.unwrap();

    let spawn = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
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
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(tell.task_key.actor, address_sup);
    assert_eq!(tell.task_key.context, address_fork);
    assert_eq!(tell.envelope.info().to, address_sup);
    let envelope = tell.take_envelope();

    recv.resolve(Ok(envelope));

    let link = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Link>()
        .unwrap();
    assert_eq!(link.task_key.actor, address_sup);
    assert_eq!(link.task_key.context, address_sup);
    assert_eq!(link.peer, address_child);
    link.resolve(());

    let _recv = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Recv>()
        .unwrap();

    tell.resolve(Ok(()));

    let mut tell = rt
        .next_event()
        .await
        .unwrap()
        .unwrap()
        .convert::<query::Tell>()
        .unwrap();
    assert_eq!(tell.task_key.actor, address_sup);
    assert_eq!(tell.task_key.context, address_fork);
    let envelope = tell.take_envelope();
    assert_eq!(envelope.info().to, address_client);
    let (response, _envelope) = envelope
        .cast::<mm1_proto_ask::Response<mm1_proto_sup::uniform::StartResponse>>()
        .unwrap()
        .take();
    assert_eq!(response.payload.unwrap(), address_child);
}
