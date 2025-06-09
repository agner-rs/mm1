use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use mm1_common::log;
use tokio::net::{TcpSocket, TcpStream};
use tokio::time;

const LISTENER_BACKLOG: u32 = 32;

pub struct RendezVous {
    pub bind:          SocketAddr,
    pub peer:          SocketAddr,
    pub delay_initial: Duration,
    pub delay_max:     Duration,
}

impl RendezVous {
    pub async fn run(&self) -> Result<TcpStream, io::Error> {
        log::debug!(
            "running rendez-vous [this: {}; peer: {}; init-delay: {:?}; max-delay: {:?}]",
            self.bind,
            self.peer,
            self.delay_initial,
            self.delay_max
        );

        if self.bind.is_ipv4() != self.bind.is_ipv4() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "both sides should be either Ipv4 or Ipv6",
            ))
        }

        let stream = tokio::select! {
            accepted = self.accept() => accepted.inspect_err(|e| log::warn!("accept error: {}", e))?,
            connected = self.connect() => connected.inspect_err(|e| log::warn!("connect error: {}", e))?,
        };

        Ok(stream)
    }
}

impl RendezVous {
    async fn connect(&self) -> Result<TcpStream, io::Error> {
        log::debug!("connecting [this: {}; peer: {}]", self.bind, self.peer);
        let mut delay = self.delay_initial;
        loop {
            let socket = new_socket(self.bind)?;
            if let Ok(connected) = socket
                .connect(self.peer)
                .await
                .inspect_err(|e| log::debug!("connect failed: {}", e))
            {
                log::debug!("connected [this: {}; peer: {}]", self.bind, self.peer);
                break Ok(connected)
            }

            time::sleep(delay).await;
            delay = self.delay_max.min(delay * 2);
        }
    }

    async fn accept(&self) -> Result<TcpStream, io::Error> {
        log::debug!("accepting [this: {}; peer: {}]", self.bind, self.peer);
        let l_socket =
            new_socket(self.bind).inspect_err(|e| log::warn!("new_socket failed: {}", e))?;
        let listener = l_socket
            .listen(LISTENER_BACKLOG)
            .inspect_err(|e| log::warn!("l_socket.bind failed: {}", e))?;
        loop {
            let (accepted, peer_addr) = listener
                .accept()
                .await
                .inspect_err(|e| log::error!("accept failed: {}", e))?;
            if peer_addr == self.peer {
                log::debug!("accepted [this: {}; peer: {}]", self.bind, self.peer);
                break Ok(accepted)
            }
            log::warn!(
                "unexpected peer-addr [this: {}; expected-peer: {}; actual-peer: {}]",
                self.bind,
                self.peer,
                peer_addr
            );
        }
    }
}

fn new_socket(address: SocketAddr) -> Result<TcpSocket, io::Error> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    socket.set_reuseaddr(true)?;
    socket.set_reuseport(true)?;
    socket.bind(address)?;

    Ok(socket)
}
