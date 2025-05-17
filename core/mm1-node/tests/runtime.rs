use core::fmt;
use std::sync::Arc;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::log::info;
use mm1_core::context::{Call, Fork, Recv, Tell, TryCall};
use mm1_core::envelope::dispatch;
use mm1_node::runtime::{Local, Rt};
use mm1_proto::message;
use mm1_proto_system::{Exit, Exited, InitAck, SpawnRequest, TrapExit};
use tokio::runtime::Runtime;
use tokio::sync::{oneshot, Notify};

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![LogTargetConfig {
            path:  vec!["*".into()],
            level: Level::TRACE,
        }],
    }
}

#[test]
fn main_actor_is_executed() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<()>();
    async fn main<C>(_ctx: &mut C, tx: oneshot::Sender<()>) {
        info!("I'm main!");
        tx.send(()).expect("tx.send");
        info!("Bye!");
    }
    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor((main, (tx,)))).expect("rt.run");
    Runtime::new().unwrap().block_on(rx).expect("rx.await");
}

#[test]
fn main_actor_forks() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<()>();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<()>)
    where
        C: TryCall<Local, SpawnRequest<Local>, CallOk = Address>,
    {
        info!("I'm main!");

        let (tx_next, rx_next) = oneshot::channel();
        let runnable = Local::actor((child, (tx_next,)));
        let _ = ctx
            .call(
                Local,
                SpawnRequest::<Local> {
                    runnable,
                    ack_to: None,
                    link_to: Default::default(),
                },
            )
            .await;
        rx_next.await.expect("rx_next.expect");
        tx.send(()).expect("tx.send");
    }
    async fn child<C>(_ctx: &mut C, tx_next: oneshot::Sender<()>) {
        info!("I'm child!");

        tx_next.send(()).expect("tx_next.send");
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor((main, (tx,)))).expect("rt.run");
    Runtime::new().unwrap().block_on(rx).expect("rx.await");
}

#[test]
fn main_actor_panics() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(_ctx: &mut C) {
        info!("I'm main!");

        panic!("I have to, really");
    }
    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor(main)).expect("rt.run");
}

#[test]
fn child_actor_panics() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<Exited>)
    where
        C: Recv + Call<Local, TrapExit> + TryCall<Local, SpawnRequest<Local>, CallOk = Address>,
    {
        info!("I'm main!");

        ctx.call(Local, TrapExit { enable: true }).await;
        let _ = ctx
            .call(
                Local,
                SpawnRequest::<Local> {
                    runnable: Local::actor(child),
                    ack_to:   None,
                    link_to:  vec![ctx.address()],
                },
            )
            .await;

        let _ = dispatch!(match ctx.recv().await.expect("ctx.recv") {
            exited @ Exited { .. } => tx.send(exited),
        });
    }
    async fn child<C>(_ctx: &mut C) {
        info!("I'm child!");

        panic!("I have to")
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor((main, (tx,)))).expect("rt.run");
    Runtime::new().unwrap().block_on(rx).expect("rx.await");
}

#[test]
fn message_is_sent_and_received() {
    let _ = mm1_logger::init(&logger_config());

    #[derive(Debug)]
    #[message]
    struct Request {
        reply_to: Address,
    }
    #[derive(Debug)]
    #[message]
    struct Response;

    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<()>)
    where
        C: Recv + TryCall<Local, SpawnRequest<Local>, CallOk = Address> + Tell,
    {
        info!("I'm main!");

        let _ = ctx
            .call(
                Local,
                SpawnRequest {
                    runnable: Local::actor(child),
                    ack_to:   Some(ctx.address()),
                    link_to:  vec![ctx.address()],
                },
            )
            .await;
        let child_address = dispatch!(match ctx.recv().await.expect("recv") {
            InitAck { address } => address,
        });
        let _ = ctx
            .tell(
                child_address,
                Request {
                    reply_to: ctx.address(),
                },
            )
            .await;
        dispatch!(match ctx.recv().await.expect("recv") {
            Response => (),
        });
        tx.send(()).expect("tx.send");
    }

    async fn child<C>(ctx: &mut C)
    where
        C: Recv + Call<Local, InitAck> + Tell,
    {
        info!("I'm child!");

        ctx.call(
            Local,
            InitAck {
                address: ctx.address(),
            },
        )
        .await;

        let reply_to = dispatch!(match ctx.recv().await.expect("recv") {
            Request { reply_to } => reply_to,
        });
        let _ = ctx.tell(reply_to, Response).await;
    }

    let (tx, rx) = oneshot::channel();
    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor((main, (tx,)))).expect("rt.run");
    Runtime::new().unwrap().block_on(rx).expect("rx.await");
}

#[test]
fn child_actor_force_exit_with_trapexit() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<Exited>)
    where
        C: Recv
            + Call<Local, TrapExit>
            + Call<Local, Exit>
            + TryCall<Local, SpawnRequest<Local>, CallOk = Address>,
        C::CallError: fmt::Debug,
    {
        info!("I'm main!");

        ctx.call(Local, TrapExit { enable: true }).await;
        let _ = ctx
            .call(
                Local,
                SpawnRequest::<Local> {
                    runnable: Local::actor(child),
                    ack_to:   Some(ctx.address()),
                    link_to:  vec![ctx.address()],
                },
            )
            .await
            .expect("start-request->outcome");

        let child_addr = dispatch!(match ctx.recv().await.expect("ctx.recv") {
            InitAck { address } => address,
        });

        ctx.call(Local, Exit { peer: child_addr }).await;

        let _ = dispatch!(match ctx.recv().await.expect("ctx.recv") {
            exited @ Exited { peer, .. } if *peer == child_addr => tx.send(exited),
        });
    }
    async fn child<C>(ctx: &mut C)
    where
        C: Recv + Call<Local, InitAck>,
    {
        info!("I'm child!");

        ctx.call(
            Local,
            InitAck {
                address: ctx.address(),
            },
        )
        .await;
        std::future::pending().await
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(Local::actor((main, (tx,)))).expect("rt.run");
    Runtime::new().unwrap().block_on(rx).expect("rx.await");
}

#[test]
fn actor_fork_run() {
    async fn main<C>(ctx: &mut C)
    where
        C: Recv + Tell + Fork,
    {
        #[derive(Debug)]
        #[message]
        struct Hello(usize);

        let main_address = ctx.address();

        let notify = Arc::new(Notify::new());

        for i in 0..10 {
            let sp = ctx.fork().await.expect("fork error");
            let notify = notify.clone();
            sp.run(move |mut ctx| {
                async move {
                    notify.notified().await;
                    ctx.tell(main_address, Hello(i))
                        .await
                        .expect("this is fine");
                }
            })
            .await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
        notify.notify_waiters();

        for _i in 0..10 {
            dispatch!(match ctx.recv().await.expect("recv") {
                Hello(idx) => eprintln!("- {}", idx),
            });
        }
    }

    Rt::create(Default::default())
        .expect("Rt::create")
        .run(Local::actor(main))
        .expect("Rt::run");
}
