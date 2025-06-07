use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::types::Never;
use mm1_core::context::{Fork, InitDone, Messaging, Quit, Start, Stop, Tell};
use mm1_core::envelope::dispatch;
use mm1_node::runtime::Rt;
use mm1_node::runtime::config::Mm1Config;
use mm1_proto::message;
use mm1_runnable::local::{self, BoxedRunnable};

#[test]
fn hello_runtime() {
    let config: Mm1Config = serde_yaml::from_str(
        r#"
            subnet: <aaaabbbbcccc:>/48
            actor_netmask: 56
            actor_inbox_size: 1024
        "#,
    )
    .expect("parse-config error");
    eprintln!("config: {:#?}", config);
    let rt = Rt::create(config).unwrap();
    rt.run(local::boxed_from_fn(main))
        .expect("main actor run error");
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Fork + Messaging + Start<BoxedRunnable<Ctx>> + Quit + InitDone + Stop + Sync,
{
    eprintln!("Hello! I'm the-main! [addr: {}]", ctx.address());

    let mut idxs = (0..).cycle();
    let mut addresses = vec![];

    while let Ok(started_address) = ctx
        .spawn(
            local::boxed_from_fn((child, (idxs.next().unwrap(),))),
            false,
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

    tokio::time::sleep(Duration::from_millis(10)).await
}

async fn child<Ctx>(ctx: &mut Ctx, idx: usize) -> Never
where
    Ctx: Quit + Messaging + InitDone + Stop,
{
    eprintln!("* Hello! I'm [{:>3}]. I live at {}", idx, ctx.address());
    tokio::task::yield_now().await;
    ctx.init_done(ctx.address()).await;
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
    let _ = ctx.kill(main_address).await;

    ctx.quit_ok().await
}

#[derive(Debug)]
#[message]
struct Request {
    reply_to: Address,
    message:  String,
}

#[derive(Debug)]
#[message]
struct Response;

#[derive(Debug)]
#[message]
struct ImMain {
    address: Address,
}
