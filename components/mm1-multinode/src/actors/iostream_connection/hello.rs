use std::pin::pin;
use std::{io, mem};

use eyre::Context;
use futures::future;
use mm1_common::types::AnyError;
use mm1_proto_network_management as nm;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{config, iostream_util, pdu};

const HELLO: &[u8] = b"MM1!";
const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

pub(super) async fn run<IO>(io: IO, options: &nm::Options) -> Result<HandshakeDone, AnyError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let io = pin!(io);
    let (mut io_r, mut io_w) = tokio::io::split(io);

    let config: config::Config = options
        .clone()
        .deserialize_into()
        .wrap_err("options.deserialize (Config)")?;
    let local_capabilities = capabilities(&config).wrap_err("capabilities from config")?;

    let inbound_header = async {
        iostream_util::read_header(&mut io_r)
            .await
            .wrap_err("read_header (Hello)")
    };
    let outbound_header_sent = async {
        let mut outbound_hello = [0u8; 30];
        let mut outbound_hello_w = io::Cursor::new(&mut outbound_hello[..]);
        outbound_hello_w
            .write_all(HELLO)
            .await
            .expect("should be fine");
        bincode::serde::encode_into_std_write(
            &local_capabilities,
            &mut outbound_hello_w,
            BINCODE_CONFIG,
        )
        .wrap_err("capabilities write error")?;

        iostream_util::write_header(&mut io_w, pdu::Hello(outbound_hello))
            .await
            .wrap_err("write_header (Hello)")?;

        io_w.flush().await?;

        Ok(())
    };

    let (inbound_header, ()) = future::try_join(inbound_header, outbound_header_sent).await?;
    let pdu::Header::Hello(pdu::Hello(inbound_hello)) = inbound_header else {
        return Err(eyre::format_err!(
            "unexpected header type: {:?}",
            inbound_header
        ));
    };
    if &inbound_hello[0..HELLO.len()] != HELLO {
        return Err(eyre::format_err!(
            "unexpected header value: {:?}",
            inbound_hello
        ));
    }
    let mut hello_data_r = io::Cursor::new(&inbound_hello[HELLO.len()..]);
    let peer_capabilities: Capabilities =
        bincode::serde::decode_from_std_read(&mut hello_data_r, BINCODE_CONFIG)
            .wrap_err("bincde::decode (Capabilities)")?;

    let mut should_be_zeroes = Vec::with_capacity(inbound_hello.len());
    hello_data_r
        .read_to_end(&mut should_be_zeroes)
        .await
        .wrap_err("hello_data_r.read_to_end")?;
    if !should_be_zeroes.into_iter().all(|b| b == 0) {
        return Err(eyre::format_err!(
            "stray data in HELLO: the peer probably has different view on Capabilities"
        ));
    }

    let done = negotiate(
        &mut io_r,
        &mut io_w,
        &config,
        local_capabilities,
        peer_capabilities,
    )
    .await
    .wrap_err("negotiate")?;

    Ok(done)
}

fn capabilities(config: &config::Config) -> Result<Capabilities, AnyError> {
    let config::Config { authc } = config;
    let authc = {
        use config::AuthcConfig::*;
        match authc {
            Trusted => CapAuthc::Trusted,
            Cookie { .. } => CapAuthc::Cookie,
            SharedSecret { .. } => CapAuthc::SharedSecret,
        }
    };
    let caps = Capabilities { authc };
    Ok(caps)
}

async fn negotiate<R, W>(
    mut io_r: R,
    mut io_w: W,
    config: &config::Config,
    local_capabilities: Capabilities,
    peer_capabilities: Capabilities,
) -> Result<HandshakeDone, AnyError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    negotiate_authc(
        &mut io_r,
        &mut io_w,
        config,
        &local_capabilities,
        &peer_capabilities,
    )
    .await
    .wrap_err("negotiate_authc")?;

    let () = negotiate_done(io_r, io_w)
        .await
        .wrap_err("negotiate_done")?;
    Ok(HandshakeDone {})
}

async fn negotiate_authc<R, W>(
    io_r: R,
    io_w: W,
    config: &config::Config,
    local_capabilities: &Capabilities,
    peer_capabilities: &Capabilities,
) -> Result<(), AnyError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut io_r = pin!(io_r);
    let mut io_w = pin!(io_w);

    let Capabilities { authc: local, .. } = local_capabilities;
    let Capabilities { authc: peer, .. } = peer_capabilities;
    let config::Config { authc } = config;

    if local != peer {
        return Err(eyre::format_err!(
            "incompatible authc-methods [local: {:?}; peer: {:?}]",
            local,
            peer
        ))
    }

    assert_eq!(local, peer);
    {
        use config::AuthcConfig::*;
        match authc {
            Trusted => Ok(()),
            Cookie(cookie) => {
                let cookie = cookie.as_ref();
                let r = async {
                    let negotiate = read_negotiate(&mut io_r).await.wrap_err("read_negotiate")?;
                    let Negotiate::Cookie(cookie_len) = negotiate else {
                        return Err(eyre::format_err!(
                            "expected Cookie; received: {:?}",
                            negotiate
                        ))
                    };
                    let mut buf = vec![0u8; cookie_len as usize];
                    io_r.read_exact(&mut buf[..])
                        .await
                        .wrap_err("io_r.read_exact (cookie)")?;
                    if &buf[..] != cookie.as_bytes() {
                        return Err(eyre::format_err!("cookie mismatch"))
                    }
                    Ok(())
                };
                let w = async {
                    let negotiate =
                        Negotiate::Cookie(cookie.len().try_into().wrap_err("cookie too long?")?);
                    write_negotiate(&mut io_w, negotiate)
                        .await
                        .wrap_err("write_negotiate")?;
                    io_w.write_all(cookie.as_bytes())
                        .await
                        .wrap_err("write cookie")?;
                    Ok(())
                };

                future::try_join(r, w).await.map(|((), ())| ())
            },
            SharedSecret { .. } => Err(eyre::format_err!("not implemented: shared-secret")),
        }
    }
}

async fn negotiate_done<R, W>(io_r: R, io_w: W) -> Result<(), AnyError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let w = async {
        write_negotiate(io_w, Negotiate::Done)
            .await
            .wrap_err("write_negotiate")?;
        Ok(())
    };
    let r = async {
        match read_negotiate(io_r).await.wrap_err("read_negotiate")? {
            Negotiate::Done => Ok(()),
            unexpected => {
                Err(eyre::format_err!(
                    "expected Done, received: {:?}",
                    unexpected
                ))
            },
        }
    };

    future::try_join(w, r).await.map(|((), ())| ())
}

async fn read_negotiate<R>(io_r: R) -> Result<Negotiate, AnyError>
where
    R: AsyncRead + Unpin,
{
    let mut io_r = pin!(io_r);
    let mut buf = [0u8; mem::size_of::<Negotiate>() * 2];
    io_r.read_exact(&mut buf[..])
        .await
        .wrap_err("io_r.read_exact")?;
    let mut buf_r = io::Cursor::new(&buf);
    let negotiate = bincode::serde::decode_from_std_read(&mut buf_r, BINCODE_CONFIG)
        .wrap_err("bincode::decode (Negotiate)")?;
    Ok(negotiate)
}

async fn write_negotiate<W>(io_w: W, negotiate: Negotiate) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let mut io_w = pin!(io_w);
    let mut buf = [0u8; mem::size_of::<Negotiate>() * 2];
    let mut buf_w = io::Cursor::new(&mut buf[..]);
    bincode::serde::encode_into_std_write(negotiate, &mut buf_w, BINCODE_CONFIG)
        .wrap_err("bincode::encode (Negotiate)")?;
    io_w.write_all(&buf[..]).await.wrap_err("io_w.write_all")?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
struct Capabilities {
    authc: CapAuthc,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum CapAuthc {
    #[default]
    Trusted      = 0,
    Cookie       = 1,
    SharedSecret = 2,
}

#[must_use]
pub(super) struct HandshakeDone {}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
enum Negotiate {
    Cookie(u16),
    Done,
}
