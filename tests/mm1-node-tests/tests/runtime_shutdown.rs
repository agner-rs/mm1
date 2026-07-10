use std::net::{SocketAddr, TcpListener};

use mm1::core::context::Start;
use mm1::runnable::local;
use mm1::runtime::config::Mm1NodeConfig;
use mm1::runtime::{Local, Rt};
use tokio::sync::oneshot;

#[test]
fn run_stops_unlinked_children_before_returning() {
    assert_run_releases_child_resources(Mm1NodeConfig::default());
}

#[test]
fn run_stops_unlinked_children_on_a_named_runtime() {
    let config = serde_yaml_ng::from_str(
        r#"
            actor:
                runtime: background
            runtime:
                background: {}
        "#,
    )
    .expect("parse config");
    assert_run_releases_child_resources(config);
}

fn assert_run_releases_child_resources(config: Mm1NodeConfig) {
    let reservation = TcpListener::bind(("127.0.0.1", 0)).expect("reserve loopback address");
    let listen_address = reservation.local_addr().expect("reserved local address");
    drop(reservation);

    let rt = Rt::create(config).expect("Rt::create");
    rt.run(local::boxed_from_fn((main, (listen_address,))))
        .expect("Rt::run");

    TcpListener::bind(listen_address)
        .expect("Rt::run must stop unlinked children and release their resources");
}

async fn main<Ctx>(ctx: &mut Ctx, listen_address: SocketAddr)
where
    Ctx: Start<Local>,
{
    let (ready_tx, ready_rx) = oneshot::channel();
    ctx.spawn(
        local::boxed_from_fn((child, (listen_address, ready_tx))),
        false,
    )
    .await
    .expect("spawn child");
    ready_rx.await.expect("child ready");
}

async fn child<Ctx>(_ctx: &mut Ctx, listen_address: SocketAddr, ready_tx: oneshot::Sender<()>) {
    let listener = TcpListener::bind(listen_address).expect("child bind");
    ready_tx.send(()).expect("report child ready");

    std::future::pending::<()>().await;
    drop(listener);
}
