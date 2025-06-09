use std::net::SocketAddr;
use std::time::Duration;

use mm1_common::types::AnyError;
use mm1_multinode::remote_subnet::tcp_rendez_vous::RendezVous;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let _ = dotenv::dotenv();
    let _ = mm1_logger::init(&logger_config());

    let a_bind: SocketAddr = std::env::var("A_BIND").unwrap().parse().unwrap();
    let b_bind: SocketAddr = std::env::var("B_BIND").unwrap().parse().unwrap();
    let a_peer: SocketAddr = std::env::var("A_PEER").unwrap().parse().unwrap();
    let b_peer: SocketAddr = std::env::var("B_PEER").unwrap().parse().unwrap();

    let a = RendezVous {
        bind:          a_bind,
        peer:          a_peer,
        delay_initial: Duration::from_millis(100),
        delay_max:     Duration::from_secs(1),
    };

    let b = RendezVous {
        bind:          b_bind,
        peer:          b_peer,
        delay_initial: Duration::from_millis(100),
        delay_max:     Duration::from_secs(1),
    };

    let (a, b) = tokio::try_join!(a.run(), b.run())?;

    eprintln!("A: {:?}", a);
    eprintln!("B: {:?}", b);

    Ok(())
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec!["*=INFO".parse().unwrap()],
    }
}
