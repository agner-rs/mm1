use std::time::Duration;

use mm1::address::Address;
use mm1::common::{Never, log};
use mm1::core::context::{Fork, InitDone, Messaging, Quit, Start, Stop, Tell};
use mm1::core::envelope::dispatch;
use mm1::proto::message;
use mm1::runnable::local::{self, BoxedRunnable};
use mm1::runtime::Rt;
use mm1::runtime::config::Mm1NodeConfig;

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
fn hello_runtime() {
    let _ = mm1_logger::init(&logger_config());

    let config: Mm1NodeConfig = serde_yaml::from_str(
        r#"
            local_subnets:
                - net: <aaaabbbbcccc:>/52
                  kind: auto
                - net: <aaaabbbbcccd:>/52
                  kind: bind
        "#,
    )
    .expect("parse-config error");
    log::info!("config: {config:#?}");
    let rt = Rt::create(config).unwrap();
    rt.run(local::boxed_from_fn(main))
        .expect("main actor run error");
}

async fn main<Ctx>(ctx: &mut Ctx)
where
    Ctx: Fork + Messaging + Start<BoxedRunnable<Ctx>> + Quit + InitDone + Stop + Sync,
{
    log::info!("Hello! I'm the-main! [addr: {}]", ctx.address());

    let mut idxs = (0..).cycle();
    let mut addresses = vec![];

    while let Ok(started_address) = ctx
        .spawn(
            local::boxed_from_fn((child, (idxs.next().unwrap(),))),
            false,
        )
        .await
    {
        log::info!("- {started_address}");
        addresses.push(started_address);
        tokio::task::yield_now().await
    }

    log::info!("spawned {} actors", addresses.len());

    for address in addresses {
        let mut rq_ctx = ctx.fork().await.expect("fork");
        let _ = rq_ctx
            .tell(
                address,
                Request {
                    reply_to: rq_ctx.address(),
                    message:  format!("Hello you {address}"),
                },
            )
            .await;
        dispatch!(match rq_ctx.recv().await.expect("recv") {
            Response { .. } => (),
        });
    }

    tokio::time::sleep(Duration::from_millis(10)).await;

    log::info!("the-main says Bye!");
}

async fn child<Ctx>(ctx: &mut Ctx, idx: usize) -> Never
where
    Ctx: Quit + Messaging + InitDone + Stop,
{
    log::info!("* Hello! I'm [{:>3}]. I live at {}", idx, ctx.address());
    tokio::task::yield_now().await;
    ctx.init_done(ctx.address()).await;
    dispatch!(match ctx.recv().await.expect("no message") {
        Request { reply_to, message } => {
            log::info!("  [{idx:>3}] received: {message:?} [from: {reply_to}]");
            let _ = ctx.tell(reply_to, Response).await;
        },
    });

    let main_address = dispatch!(match ctx.recv().await.expect("no message") {
        ImMain { address } => address,
    });
    log::info!("Sending StopRequest to {main_address}");
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
