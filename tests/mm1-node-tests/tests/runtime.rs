use std::mem;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use mm1::address::{Address, AddressPool, NetAddress, NetMask};
use mm1::common::log::{self, info};
use mm1::core::context::{
    Bind, BindArgs, Fork, InitDone, Linking, Messaging, Quit, Start, Stop, Tell, Watching,
};
use mm1::core::envelope::dispatch;
use mm1::proto::message;
use mm1::proto::system::Exited;
use mm1::runnable::local;
use mm1::runtime::{Local, Rt};
use tokio::sync::{Notify, oneshot};

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
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");
    rx.now_or_never().unwrap().expect("rx.await");
}

#[test]
fn main_actor_forks() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel::<()>();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<()>)
    where
        C: Start<Local>,
    {
        info!("I'm main!");

        let (tx_next, rx_next) = oneshot::channel();
        let runnable = local::boxed_from_fn((child, (tx_next,)));
        let _ = ctx.spawn(runnable, false).await;
        rx_next.await.expect("rx_next.expect");
        tx.send(()).expect("tx.send");
    }
    async fn child<C>(_ctx: &mut C, tx_next: oneshot::Sender<()>) {
        info!("I'm child!");

        tx_next.send(()).expect("tx_next.send");
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");
    rx.now_or_never().unwrap().expect("rx.await");
}

#[test]
fn main_actor_panics() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(_ctx: &mut C) {
        info!("I'm main!");

        panic!("I have to, really");
    }
    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn(main)).expect("rt.run");
}

#[test]
fn child_actor_panics() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<Exited>)
    where
        C: Messaging + Linking + Start<Local>,
    {
        info!("I'm main!");

        ctx.set_trap_exit(true).await;
        let _ = ctx
            .spawn(local::boxed_from_fn(child), true)
            .await
            .expect("spawn failed");

        let _ = dispatch!(match ctx.recv().await.expect("ctx.recv") {
            exited @ Exited { .. } => tx.send(exited),
        });
    }
    async fn child<C>(_ctx: &mut C) {
        info!("I'm child!");

        panic!("I have to")
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");
    rx.now_or_never().unwrap().expect("rx.await");
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
        C: Messaging + Start<Local>,
    {
        info!("I'm main!");

        let child_address = ctx
            .start(local::boxed_from_fn(child), true, Duration::from_secs(1))
            .await
            .expect("start failed");
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
        C: Messaging + InitDone,
    {
        info!("I'm child!");

        ctx.init_done(ctx.address()).await;

        let reply_to = dispatch!(match ctx.recv().await.expect("recv") {
            Request { reply_to } => reply_to,
        });
        let _ = ctx.tell(reply_to, Response).await;
    }

    let (tx, rx) = oneshot::channel();
    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");
    rx.now_or_never().unwrap().expect("rx.await");
}

#[test]
fn child_actor_force_exit_with_trapexit() {
    let _ = mm1_logger::init(&logger_config());

    let (tx, rx) = oneshot::channel();
    async fn main<C>(ctx: &mut C, tx: oneshot::Sender<Exited>)
    where
        C: Messaging + Linking + Quit + Start<Local> + Stop,
    {
        info!("I'm main!");

        ctx.set_trap_exit(true).await;
        let child_addr = ctx
            .start(local::boxed_from_fn(child), true, Duration::from_secs(1))
            .await
            .expect("start failed");
        ctx.exit(child_addr).await;

        let _ = dispatch!(match ctx.recv().await.expect("ctx.recv") {
            exited @ Exited { peer, .. } if *peer == child_addr => tx.send(exited),
        });
    }
    async fn child<C>(ctx: &mut C)
    where
        C: Messaging + InitDone,
    {
        info!("I'm child!");

        ctx.init_done(ctx.address()).await;
        std::future::pending().await
    }

    let rt = Rt::create(Default::default()).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (tx,)))).expect("rt.run");
    rx.now_or_never().unwrap().expect("rx.await");
}

#[test]
fn actor_fork_run() {
    async fn main<C>(ctx: &mut C)
    where
        C: Messaging + Fork,
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
        .run(local::boxed_from_fn(main))
        .expect("Rt::run");
}

#[test]
fn actor_bind_and_recv() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Messaging + Fork + Bind<NetAddress>,
    {
        #[derive(Debug)]
        #[message]
        struct Hello;

        log::info!("hello!");

        let bind_to = "<beaf:>/16".parse().unwrap();
        let pool = AddressPool::new(bind_to);
        let () = ctx
            .bind(BindArgs {
                bind_to,
                inbox_size: 1024,
            })
            .await
            .expect("bind");

        const COUNT: usize = 100;

        for _ in 0..COUNT {
            let to = pool.lease(NetMask::M_64).expect("pool.lease");
            ctx.tell(to.address, Hello).await.expect("ctx.tell");
        }

        for _ in 0..COUNT {
            let envelope = ctx.recv().await.expect("bound.recv");
            log::info!("received [dest: {}]", envelope.header().to);
        }
    }

    Rt::create(
        serde_yaml::from_str(
            r#"
                subnet: <cafe:>/16

            "#,
        )
        .expect("Mm1Config from yaml"),
    )
    .expect("Rt::create")
    .run(local::boxed_from_fn(main))
    .expect("Rt::run");
}

#[test]
fn actor_fork_watched_yields_down() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<C>(ctx: &mut C)
    where
        C: Messaging + Fork + Watching,
    {
        log::info!("hello!");

        let fork_ctx = ctx.fork().await.expect("ctx.fork");
        let fork_address = fork_ctx.address();

        info!("I ({}) watch for {}", ctx.address(), fork_ctx.address());

        let fork_watch_ref = ctx.watch(fork_address).await;
        mem::drop(fork_ctx);
        let envelope = ctx.recv().await.expect("ctx.recv");
        dispatch!(match envelope {
            down @ mm1::proto::system::Down {
                watch_ref, peer, ..
            } => {
                assert_eq!(watch_ref, fork_watch_ref);
                assert_eq!(peer, fork_address);
                eprintln!("{:?}", down);
            },
        })
    }

    Rt::create(
        serde_yaml::from_str(
            r#"
                subnet: <cafe:>/16
            "#,
        )
        .expect("Mm1Config from yaml"),
    )
    .expect("Rt::create")
    .run(local::boxed_from_fn(main))
    .expect("Rt::run");
}
