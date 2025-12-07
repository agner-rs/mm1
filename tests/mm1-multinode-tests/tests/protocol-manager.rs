use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use futures::FutureExt;
use insta::assert_yaml_snapshot;
use mm1::ask::Ask;
use mm1::common::error::AnyError;
use mm1::core::context::Fork;
use mm1::multinode::Protocol;
use mm1::runnable::local;
use mm1_proto_network_management::protocols;
use mm1_proto_well_known::MULTINODE_MANAGER;
use tokio::sync::oneshot;

#[test]
fn mixed_test() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<Ctx>(ctx: &mut Ctx, tx: oneshot::Sender<()>)
    where
        Ctx: Ask + Fork,
    {
        register(ctx, "proto-1", Protocol::new())
            .await
            .expect("register proto-1");

        assert_yaml_snapshot!(
            "register_proto-1_twice",
            e(register(ctx, "proto-1", Protocol::new())
                .await
                .expect_err("register proto-1 should have failed"))
        );

        register(ctx, "proto-2", Protocol::new())
            .await
            .expect("register proto-2");

        let proto_2 = get(ctx, "proto-2", None).await.expect("get proto-2");

        unregister(ctx, "proto-1")
            .await
            .expect("unregister proto-1");

        assert_yaml_snapshot!(
            "unregister_proto-1_twice",
            e(unregister(ctx, "proto-1")
                .await
                .expect_err("unregister proto-1 should have failed"))
        );

        assert_yaml_snapshot!(
            "unregister_proto-2_while_in_use",
            e(unregister(ctx, "proto-2")
                .await
                .expect_err("unregister proto-2 should have failed"))
        );
        std::mem::drop(proto_2);
        unregister(ctx, "proto-2")
            .await
            .expect("unregister proto-2");

        tx.send(()).expect("tx");
    }

    let (tx, rx) = oneshot::channel();
    mm1::runtime::Rt::create(Default::default())
        .expect("create runtime")
        .run(local::boxed_from_fn((main, (tx,))))
        .expect("run main actor");
    rx.now_or_never().unwrap().expect("rx");
}

#[test]
fn no_protocol_no_wait() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<Ctx>(ctx: &mut Ctx, tx: oneshot::Sender<()>)
    where
        Ctx: Ask + Fork,
    {
        let reason = get(ctx, "proto", None)
            .await
            .expect_err("should be no-protocol");
        assert_yaml_snapshot!("no_protocol_no_wait", e(reason));
        tx.send(()).unwrap();
    }

    let (tx, rx) = oneshot::channel();
    mm1::runtime::Rt::create(Default::default())
        .expect("create runtime")
        .run(local::boxed_from_fn((main, (tx,))))
        .expect("run main actor");
    rx.now_or_never().unwrap().expect("rx");
}

#[test]
fn no_protocol_with_timeout() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<Ctx>(ctx: &mut Ctx, tx: oneshot::Sender<()>)
    where
        Ctx: Ask + Fork,
    {
        let reason = get(ctx, "proto", Some(Duration::from_secs(1)))
            .await
            .expect_err("should be timeout");
        assert_yaml_snapshot!("no_protocol_with_timeout", e(reason));
        tx.send(()).unwrap();
    }

    let (tx, rx) = oneshot::channel();
    mm1::runtime::Rt::create(Default::default())
        .expect("create runtime")
        .run(local::boxed_from_fn((main, (tx,))))
        .expect("run main actor");
    rx.now_or_never().unwrap().expect("rx");
}

#[test]
fn get_protocol_positive_case() {
    let _ = mm1_logger::init(&logger_config());

    async fn main<Ctx>(ctx: &mut Ctx, tx: oneshot::Sender<()>) -> Result<(), AnyError>
    where
        Ctx: Ask + Fork,
    {
        let (p1_tx, p1_rx) = oneshot::channel();
        let (p2_tx, p2_rx) = oneshot::channel();

        register(ctx, "proto-1", Protocol::new()).await.unwrap();

        ctx.fork()
            .await
            .wrap_err("fork")?
            .run(async move |mut ctx| {
                p1_tx
                    .send(get(&mut ctx, "proto-1", Some(Duration::from_secs(1))).await)
                    .unwrap();
            })
            .await;

        ctx.fork()
            .await
            .wrap_err("fork")?
            .run(async move |mut ctx| {
                p2_tx
                    .send(get(&mut ctx, "proto-2", Some(Duration::from_secs(1))).await)
                    .unwrap();
            })
            .await;

        tokio::time::sleep(Duration::from_millis(250)).await;

        register(ctx, "proto-2", Protocol::new()).await.unwrap();

        tokio::time::sleep(Duration::from_millis(250)).await;

        let p1 = p1_rx.await.unwrap().expect("p1");
        let p2 = p2_rx.await.unwrap().expect("p2");

        assert_yaml_snapshot!(
            "get_protocol_positive_case--protocols-are-in-use",
            [
                e(unregister(ctx, "proto-1").await.expect_err("p1 is in use")),
                e(unregister(ctx, "proto-2").await.expect_err("p2 is in use")),
            ]
        );

        std::mem::drop((p1, p2));

        unregister(ctx, "proto-1")
            .await
            .expect("protocol is released");
        unregister(ctx, "proto-2")
            .await
            .expect("protocol is released");

        tx.send(()).unwrap();

        Ok(())
    }

    let (tx, rx) = oneshot::channel();
    mm1::runtime::Rt::create(Default::default())
        .expect("create runtime")
        .run(local::boxed_from_fn((main, (tx,))))
        .expect("run main actor");
    rx.now_or_never().unwrap().expect("rx");
}

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

fn e(e: AnyError) -> Vec<String> {
    e.chain().map(|r| r.to_string()).collect()
}

async fn register<Ctx>(ctx: &mut Ctx, name: &str, protocol: Protocol) -> Result<(), AnyError>
where
    Ctx: Ask + Fork,
{
    let () = ctx
        .ask::<_, protocols::RegisterProtocolResponse>(
            MULTINODE_MANAGER,
            protocols::RegisterProtocolRequest {
                name: name.into(),
                protocol,
            },
            Duration::from_millis(100),
        )
        .await
        .wrap_err("ask")?
        .wrap_err("register")?;
    Ok(())
}

async fn unregister<Ctx>(ctx: &mut Ctx, name: &str) -> Result<(), AnyError>
where
    Ctx: Ask + Fork,
{
    let () = ctx
        .ask::<_, protocols::UnregisterProtocolResponse>(
            MULTINODE_MANAGER,
            protocols::UnregisterProtocolRequest { name: name.into() },
            Duration::from_millis(100),
        )
        .await
        .wrap_err("ask")?
        .wrap_err("unregister")?;
    Ok(())
}

async fn get<Ctx>(
    ctx: &mut Ctx,
    name: &str,
    timeout: Option<Duration>,
) -> Result<Arc<Protocol>, AnyError>
where
    Ctx: Ask + Fork,
{
    let protocols::ProtocolResolved { protocol, .. } = ctx
        .ask::<_, protocols::GetProtocolByNameResponse<Protocol>>(
            MULTINODE_MANAGER,
            protocols::GetProtocolByNameRequest {
                name: name.into(),
                timeout,
            },
            Duration::from_secs(3),
        )
        .await
        .wrap_err("ask")?
        .wrap_err("get")?;
    Ok(protocol)
}
