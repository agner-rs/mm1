use std::io;
use std::pin::pin;

use eyre::Context;
use futures::future;
use mm1_common::types::AnyError;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use super::{pdu, util};

const HELLO: &[u8] = b"MM1.v0.2-wip";

pub(super) async fn run<IO>(io: IO) -> Result<(), AnyError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let io = pin!(io);
    let (io_r, mut io_w) = tokio::io::split(io);

    let inbound_header = async {
        util::read_header(io_r)
            .await
            .wrap_err("read_header (Hello)")
    };
    let outbound_header_sent = async {
        let mut hello_message = [0u8; 30];
        io::Cursor::new(&mut hello_message[..])
            .write_all(HELLO)
            .await
            .expect("should be fine");

        util::write_header(&mut io_w, pdu::Hello(hello_message))
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

    Ok(())
}
