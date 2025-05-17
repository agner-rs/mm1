use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::log::{debug, info};
use mm1_common::types::Never;
use mm1_core::context::{Ask, Call, Fork, Quit, Recv, Tell, TryCall};
use mm1_node::runtime::{Local, Rt};
use mm1_proto::message;
use mm1_proto_sup::uniform;
use mm1_proto_system::{
    InitAck, SpawnRequest, System, {self as system},
};
use mm1_sup::common::child_spec::{ChildSpec, ChildType, InitType};
use mm1_sup::common::factory::ActorFactoryMut;
use mm1_sup::uniform::{uniform_sup, UniformSup};

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "*=INFO".parse().unwrap(),
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

    async fn worker<Sys, Ctx>(ctx: &mut Ctx, reply_to: Address, delay: Duration) -> Never
    where
        Sys: System + Default,
        Ctx: Recv + Tell + Quit,
        Ctx: Call<Sys, system::InitAck>,
    {
        static WORKER_ID: AtomicUsize = AtomicUsize::new(0);
        let worker_id = WORKER_ID.fetch_add(1, Ordering::Relaxed);
        debug!(
            "worker[{}] started, replying to {} in {:?}",
            worker_id, reply_to, delay
        );
        ctx.call(
            Sys::default(),
            system::InitAck {
                address: ctx.address(),
            },
        )
        .await;

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
        Ctx: Fork + Recv + Tell + TryCall<Local, SpawnRequest<Local>>,
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

        let _ = ctx
            .call(
                Local,
                SpawnRequest {
                    runnable: sup_runnable,
                    ack_to:   Some(ctx.address()),
                    link_to:  vec![ctx.address()],
                },
            )
            .await;
        let (init_ack, _) = ctx
            .recv()
            .await
            .expect("recv")
            .cast::<InitAck>()
            .expect("cast")
            .take();

        let sup_addr = init_ack.address;
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
