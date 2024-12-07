use std::time::Duration;

use mm1_address::address::Address;
use mm1_core::context::{dispatch, Call, Fork, Quit, Recv, Tell, TryCall};
use mm1_core::types::Never;
use mm1_node::runtime::config::Mm1Config;
use mm1_node::runtime::{runnable, Local, Rt};
use mm1_proto::Traversable;
use mm1_proto_system::{InitAck, Kill, SpawnRequest};

#[test]
fn hello_runtime() {
    let config: Mm1Config = serde_yaml::from_str(
        r#"
            subnet_address: aaaabbbbcccc0000/48
            actor_netmask: 56
            actor_inbox_size: 1024
        "#,
    )
    .expect("parse-config error");
    eprintln!("config: {:#?}", config);
    let rt = Rt::create(config).unwrap();
    rt.run(runnable::boxed_from_fn(main))
        .expect("main actor run error");
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Recv + Fork + Tell + TryCall<Local, SpawnRequest<Local>, CallOk = Address>,
{
    eprintln!("Hello! I'm the-main! [addr: {}]", ctx.address());

    let mut idxs = (0..).cycle();
    let mut addresses = vec![];

    while let Ok(started_address) = ctx
        .call(
            Local,
            SpawnRequest {
                runnable: Local::actor((child, (idxs.next().unwrap(),))),
                ack_to:   Some(ctx.address()),
                link_to:  Default::default(),
            },
        )
        .await
    {
        eprintln!("- {}", started_address);
        addresses.push(started_address);
        tokio::task::yield_now().await
    }

    for address in addresses {
        let mut rq_ctx = ctx.fork().await.expect("fork");
        let _ = rq_ctx
            .tell(
                address,
                Request {
                    reply_to: rq_ctx.address(),
                    message:  format!("Hello you {}", address),
                },
            )
            .await;
        dispatch!(match rq_ctx.recv().await.expect("recv") {
            Response { .. } => (),
        });
    }

    while let Ok(inbound) = ctx.recv().await {
        let ack_from = dispatch!(match inbound {
            InitAck { address } => address,
        });
        eprintln!("# ACKED {}", ack_from);
        let _ = ctx
            .tell(
                ack_from,
                ImMain {
                    address: ctx.address(),
                },
            )
            .await;
    }

    tokio::time::sleep(Duration::from_millis(10)).await
}

async fn child<Ctx>(ctx: &mut Ctx, idx: usize) -> Never
where
    Ctx:
        Recv + Quit + Tell + Call<Local, InitAck, Outcome = ()> + Call<Local, Kill, Outcome = bool>,
{
    eprintln!("* Hello! I'm [{:>3}]. I live at {}", idx, ctx.address());
    tokio::task::yield_now().await;
    ctx.call(
        Local,
        InitAck {
            address: ctx.address(),
        },
    )
    .await;
    dispatch!(match ctx.recv().await.expect("no message") {
        Request { reply_to, message } => {
            eprintln!(
                "  [{:>3}] received: {:?} [from: {}]",
                idx, message, reply_to
            );
            let _ = ctx.tell(reply_to, Response).await;
        },
    });

    let main_address = dispatch!(match ctx.recv().await.expect("no message") {
        ImMain { address } => address,
    });
    eprintln!("Sending StopRequest to {}", main_address);
    let _ = ctx.call(Local, Kill { peer: main_address }).await;

    ctx.quit_ok().await
}

#[derive(Traversable)]
struct Request {
    reply_to: Address,
    message:  String,
}

#[derive(Traversable)]
struct Response;

#[derive(Traversable)]
struct ImMain {
    address: Address,
}
