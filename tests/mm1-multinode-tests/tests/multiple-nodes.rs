use std::time::Duration;

use eyre::Context;
use mm1::ask::Ask;
use mm1::common::error::AnyError;
use mm1::core::context::{Messaging, Tell};
use mm1::multinode::{MULTINODE_MANAGER, Protocol};
use mm1::runnable::local;
use mm1::runtime::Rt;
use mm1_multinode::actors::context::ActorContext;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::oneshot;

#[test]
fn two_nodes() {
    mm1_logger::init(&logger_config()).ok();
    let temp_dir = TempDir::new().expect("temp-dir");
    let socket_url = format!(
        "uds://{}",
        temp_dir
            .path()
            .join("test.sock")
            .to_str()
            .expect("path to_str")
    );

    let config_1 = json!({
        "local_subnets": [
            { "net": "<aa0:>/16", "kind": "auto" },
            { "net": "<aa1:>/16", "kind": "bind" },
        ],
        "inbound": [
            { "proto": ["proto"], "addr": socket_url, "authc": "trusted" }
        ],
    });
    let config_2 = json!({
        "local_subnets": [
            { "net": "<aa2:>/16", "kind": "auto" },
            { "net": "<aa3:>/16", "kind": "bind" },
        ],
        "outbound": [
            { "proto": ["proto"], "addr": socket_url, "authc": "trusted" }
        ],
    });

    let rt1 = Rt::create(serde_json::from_value(config_1).expect("config-1")).expect("create rt1");
    let rt2 = Rt::create(serde_json::from_value(config_2).expect("config-2")).expect("create rt2");

    mod proto {
        use mm1::proto::message;

        #[message]
        pub struct Hi;
    }

    std::thread::scope(|s| {
        let (borrow_1_tx, borrow_1_rx) = oneshot::channel();
        let (returned_1_tx, returned_1_rx) = oneshot::channel();
        s.spawn(|| rt1.run(local::boxed_from_fn((main, (borrow_1_tx, returned_1_rx)))));

        let (borrow_2_tx, borrow_2_rx) = oneshot::channel();
        let (returned_2_tx, returned_2_rx) = oneshot::channel();
        s.spawn(|| rt2.run(local::boxed_from_fn((main, (borrow_2_tx, returned_2_rx)))));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Builder::new_current_thread");
        rt.block_on(async move {
            let mut ctx_1 = borrow_1_rx.await.expect("borrow_1_rx");
            let mut ctx_2 = borrow_2_rx.await.expect("borrow_2_rx");

            let _: mm1::multinode::proto::RegisterProtocolResponse = ctx_1
                .ask(
                    MULTINODE_MANAGER,
                    mm1::multinode::proto::RegisterProtocolRequest {
                        name:     "proto".into(),
                        protocol: Protocol::new().with_type::<proto::Hi>(),
                    },
                    Duration::from_secs(1),
                )
                .await
                .expect("ctx_1.ask");

            let _: mm1::multinode::proto::RegisterProtocolResponse = ctx_2
                .ask(
                    MULTINODE_MANAGER,
                    mm1::multinode::proto::RegisterProtocolRequest {
                        name:     "proto".into(),
                        protocol: Protocol::new().with_type::<proto::Hi>(),
                    },
                    Duration::from_secs(1),
                )
                .await
                .expect("ctx_2.ask");

            tokio::time::sleep(Duration::from_secs(2)).await;

            ctx_1
                .tell(ctx_2.address(), proto::Hi)
                .await
                .expect("ctx_1.tell");
            let hi_envelope = ctx_2.recv().await.expect("ctx_2.recv");
            let (proto::Hi, _) = hi_envelope.cast().expect("hi_envelope.cast").take();

            returned_1_tx.send(ctx_1).ok();
            returned_2_tx.send(ctx_2).ok();
        });
    });
}

async fn main<Ctx>(
    ctx: &mut Ctx,
    borrow: oneshot::Sender<Ctx>,
    returned: oneshot::Receiver<Ctx>,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    let fork = ctx.fork().await.wrap_err("ctx.fork")?;
    borrow.send(fork).map_err(|_| ()).expect("borrow.send");
    let _fork = returned.await.expect("returned");

    Ok(())
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            "mm1_multinode::*=TRACE".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
