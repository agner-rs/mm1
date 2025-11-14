use std::pin::pin;

use eyre::Context;
use mm1_common::types::AnyError;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::pdu::Header;
use crate::actors::iostream_connection::pdu::{ForeignTypeKey, HEADER_FRAME_SIZE, LocalTypeKey};

pub(super) async fn read_header<R>(io: R) -> Result<Header<ForeignTypeKey>, AnyError>
where
    R: Unpin + AsyncRead,
{
    let mut io = pin!(io);
    let mut buf = [0u8; HEADER_FRAME_SIZE];

    io.read_exact(&mut buf).await.wrap_err("io.read_exact")?;
    let (header, _): (Header<ForeignTypeKey>, _) =
        bincode::serde::decode_from_slice(&buf, bincode::config::standard())
            .wrap_err("bincode::serde::decode::<Header>")?;
    Ok(header)
}

pub(super) async fn write_header<W>(
    io: W,
    header: impl Into<Header<LocalTypeKey>>,
) -> Result<(), AnyError>
where
    W: AsyncWrite,
{
    let mut io = pin!(io);
    let mut buf = [0u8; HEADER_FRAME_SIZE];
    let header = header.into();

    bincode::serde::encode_into_slice(header, &mut buf[..], bincode::config::standard())
        .wrap_err("bincode::serde::encode::<Header>")?;
    io.write_all(&buf[..]).await.wrap_err("io.write_all")?;

    Ok(())
}
