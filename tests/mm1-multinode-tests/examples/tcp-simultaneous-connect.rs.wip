use std::net::SocketAddr;
use std::time::Duration;

use mm1::common::error::AnyError;
use tokio::net::{TcpSocket, TcpStream};
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let _ = dotenv::dotenv();

    let a_bind: SocketAddr = std::env::var("A_BIND").unwrap().parse().unwrap();
    let b_bind: SocketAddr = std::env::var("B_BIND").unwrap().parse().unwrap();
    let a_peer: SocketAddr = std::env::var("A_PEER").unwrap().parse().unwrap();
    let b_peer: SocketAddr = std::env::var("B_PEER").unwrap().parse().unwrap();

    let a_run = run_side("a", a_bind, a_peer);
    let b_run = run_side("b", b_bind, b_peer);

    let (side_a_stream, side_b_stream) = tokio::try_join!(a_run, b_run)?;

    eprintln!("both sides:\n\ta: {side_a_stream:?}\n\tb: {side_b_stream:?}");

    Ok(())
}

async fn run_side(side: &str, bind: SocketAddr, peer: SocketAddr) -> Result<TcpStream, AnyError> {
    eprintln!("side: {side:?}; setting up");

    // Setup reusable listener
    let listener_socket = if bind.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    listener_socket.set_reuseaddr(true)?;
    listener_socket.set_reuseport(true)?; // Optional; required on some systems
    listener_socket.bind(bind)?;
    let listener = listener_socket.listen(1024)?; // Arbitrary backlog
    eprintln!("side: {side}; listening on {bind:?}");

    // Spawn connect attempt with reusable socket
    let connect_fut = async move {
        eprintln!("side: {side}; attempting connect to {peer:?}");
        let stream = loop {
            let connector_socket = if bind.is_ipv4() {
                TcpSocket::new_v4()?
            } else {
                TcpSocket::new_v6()?
            };
            connector_socket.set_reuseaddr(true)?;
            connector_socket.set_reuseport(true)?;
            connector_socket.bind(bind)?;
            if let Ok(connected) = connector_socket.connect(peer).await {
                break connected
            }
            time::sleep(Duration::from_millis(100)).await;
        };
        eprintln!("side: {side}; outbound connection established");
        Ok::<_, AnyError>(stream)
    };

    // Race between accept and connect
    let stream = tokio::select! {
        incoming = listener.accept() => {
            let (stream, addr) = incoming?;
            eprintln!("side: {side}; incoming connection from {addr:?}");
            Ok(stream)
        }
        outgoing = connect_fut => {
            let stream = outgoing?;
            eprintln!("side: {side}; connected to peer");
            Ok(stream)
        }
    };

    stream
}
