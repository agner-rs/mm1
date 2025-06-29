use std::any::TypeId;
use std::io;
use std::pin::pin;

use futures::{SinkExt, StreamExt};
use mm1_address::subnet::NetAddress;
use mm1_common::log;
use mm1_common::types::AnyError;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::codecs::Codec;
use crate::remote_subnet::config::SerdeFormat;

const PROTOCOL_MAGIC: &str = "MM1-WIP";
const FRAME_LEN_FIELD_LEN: usize = 2;
const MAX_FRAME_LEN: usize = 1024;

#[derive(Debug)]
pub(crate) struct RequestedCapabilities {
    pub(crate) requested_subnet: NetAddress,
    pub(crate) requested_format: Option<SerdeFormat>,
    pub(crate) requested_types:  Vec<String>,
}

#[derive(Debug)]
pub(crate) struct AdvertisedCapabilities {
    pub advertised_types: Vec<(TypeId, String)>,
}

pub(crate) async fn do_handshake<IO>(
    stream: IO,
    request_subnet: NetAddress,
    codec: &Codec,
    serde_format: SerdeFormat,
) -> Result<(RequestedCapabilities, AdvertisedCapabilities), AnyError>
where
    IO: AsyncRead + AsyncWrite,
{
    let (input, output) = tokio::io::split(stream);
    tokio::try_join!(
        reader(input, codec),
        writer(output, request_subnet, codec, serde_format),
    )
}

async fn writer<IO>(
    output: IO,
    request_subnet: NetAddress,
    codec: &Codec,
    serde_format: SerdeFormat,
) -> Result<AdvertisedCapabilities, AnyError>
where
    IO: AsyncWrite,
{
    let mut output = pin!(output);

    output.write_all(PROTOCOL_MAGIC.as_bytes()).await?;

    let mut caps = AdvertisedCapabilities {
        advertised_types: Default::default(),
    };

    let mut frame_output = FramedWrite::new(output, frame_codec());
    frame_output
        .send(Packet::NetAddress(request_subnet).to_bytes())
        .await?;

    frame_output
        .send(Packet::SerdeFormat(serde_format).to_bytes())
        .await?;

    for (tid, name) in codec.supported_types() {
        caps.advertised_types.push((tid, name.into()));
        frame_output
            .send(Packet::Type(name.into()).to_bytes())
            .await?;
    }

    frame_output.send(Packet::Done.to_bytes()).await?;

    let () = frame_output.flush().await?;

    Ok(caps)
}

async fn reader<IO>(input: IO, codec: &Codec) -> Result<RequestedCapabilities, AnyError>
where
    IO: AsyncRead,
{
    let mut input = pin!(input);
    let mut expect_magic = [0u8; PROTOCOL_MAGIC.len()];
    input.read_exact(&mut expect_magic).await?;
    if expect_magic != PROTOCOL_MAGIC.as_bytes() {
        return Err("bad magic".into())
    }

    let mut caps = RequestedCapabilities {
        requested_subnet: "<:>/0".parse().unwrap(),
        requested_format: None,
        requested_types:  Default::default(),
    };
    let mut frame_input = FramedRead::new(input, frame_codec());

    loop {
        let packet_bytes = frame_input
            .next()
            .await
            .transpose()?
            .ok_or("unexpected end of stream")?;

        let packet = Packet::from_bytes(packet_bytes.freeze())?;
        log::debug!("in-packet: {:?}", packet);
        match packet {
            Packet::Done => break,
            Packet::NetAddress(a) => caps.requested_subnet = a,
            Packet::SerdeFormat(f) => caps.requested_format = Some(f),
            Packet::Type(t) => caps.requested_types.push(t),
        }
    }

    let mut all_requested_types_are_supported = true;
    for name in &caps.requested_types {
        if codec.select_type(name).is_none() {
            log::error!("peer requested unsupported type: {}", name);
            all_requested_types_are_supported = false
        }
    }

    if !all_requested_types_are_supported {
        return Err("peer requested unsupported type".into())
    }

    Ok(caps)
}

fn frame_codec()
-> impl Encoder<Bytes, Error = io::Error> + Decoder<Item = BytesMut, Error = io::Error> {
    LengthDelimitedCodec::builder()
        .length_field_length(FRAME_LEN_FIELD_LEN)
        .big_endian()
        .max_frame_length(MAX_FRAME_LEN)
        .new_codec()
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Packet {
    NetAddress(NetAddress),
    SerdeFormat(SerdeFormat),
    Type(String),
    Done,
}

impl Packet {
    fn to_bytes(&self) -> Bytes {
        let vec = serde_json::to_vec(self).expect("serde encode failed");
        Bytes::from_owner(vec)
    }

    fn from_bytes(bytes: Bytes) -> Result<Self, AnyError> {
        let packet = serde_json::from_slice(bytes.as_ref())?;
        Ok(packet)
    }
}
