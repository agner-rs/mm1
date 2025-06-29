use std::any::TypeId;
use std::collections::HashMap;
use std::io;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::log;
use mm1_common::types::{AnyError, Never};
use mm1_core::context::{Bind, BindArgs, Fork, InitDone, Messaging};
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::codecs::{self, CodecRegistry};
use crate::remote_subnet::config::{RemoteSubnetConfig, SerdeFormat};
use crate::remote_subnet::handshake;
use crate::remote_subnet::tcp_rendez_vous::RendezVous;

const SUBNET_HANDLER_INBOX_SIZE: usize = 1024;
const RENDEZ_VOUS_DELAY_INITIAL: Duration = Duration::from_millis(100);
const RENDEZ_VOUS_DELAY_MAX: Duration = Duration::from_secs(5);

const FRAME_LEN_FIELD_LEN: usize = 4;
const MAX_FRAME_LEN: usize = 64 * 1024;

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    codec_registry: Arc<CodecRegistry>,
    net_address: NetAddress,
    config: Arc<RemoteSubnetConfig>,
) -> Result<Never, AnyError>
where
    Ctx: Bind<NetAddress> + Fork + InitDone + Messaging,
{
    run_inner(ctx, codec_registry, net_address, config)
        .await
        .inspect_err(|reason| log::error!("subnet {} failure: {}", net_address, reason))
}

async fn run_inner<Ctx>(
    ctx: &mut Ctx,
    codec_registry: Arc<CodecRegistry>,
    net_address: NetAddress,
    config: Arc<RemoteSubnetConfig>,
) -> Result<Never, AnyError>
where
    Ctx: Bind<NetAddress> + Fork + InitDone + Messaging,
{
    log::info!(
        "starting remote subnet {} with config: {:?}",
        net_address,
        config
    );
    let codec = {
        let codec_name = match config.as_ref() {
            RemoteSubnetConfig::Wip(c) => c.codec.as_str(),
        };
        codec_registry
            .get_codec(codec_name)
            .ok_or_else(|| format!("no such codec: {}", codec_name))?
    };

    let mut subnet_ctx = ctx.fork().await?;
    subnet_ctx
        .bind(BindArgs {
            bind_to:    net_address,
            inbox_size: SUBNET_HANDLER_INBOX_SIZE,
        })
        .await?;

    ctx.init_done(ctx.address()).await;

    let RemoteSubnetConfig::Wip(config) = config.as_ref();

    let rendez_vous = RendezVous {
        bind:          config.link.bind,
        peer:          config.link.peer,
        delay_initial: RENDEZ_VOUS_DELAY_INITIAL,
        delay_max:     RENDEZ_VOUS_DELAY_MAX,
    };

    let mut stream = rendez_vous.run().await?;
    log::debug!("stream open: {:?}", stream);

    let (requested_caps, advertised_caps) =
        handshake::do_handshake(&mut stream, net_address, &config.authc, codec, config.serde)
            .await?;
    log::debug!("handshake done");

    if requested_caps
        .requested_format
        .ok_or("peer has not requested serde-format")?
        != config.serde
    {
        log::error!(
            "could not agree on serde-format [this: {:?}; peer: {:?}]",
            config.serde,
            requested_caps.requested_format
        );
        return Err("serde-format mismatch".into())
    }

    let mut encoders = HashMap::new();
    for (type_idx, (tid, name)) in advertised_caps.advertised_types.into_iter().enumerate() {
        let supported_type = codec
            .select_type(&name)
            .expect("we ourselves advertised it in the handshake, haven't we?");
        let encoder = supported_type.select_format(config.serde);
        encoders.insert(tid, (type_idx, encoder));
    }

    let mut decoders = vec![];
    for name in requested_caps.requested_types {
        let supported_type = codec
            .select_type(&name)
            .expect("we checked every required type in the handshake");
        let decoder = supported_type.select_format(config.serde);
        decoders.push(decoder);
    }

    let (input, output) = tokio::io::split(&mut stream);

    let outbound_running = handle_outbound(subnet_ctx, output, net_address, config.serde, encoders);
    let inbound_running = handle_inbound(ctx, input, config.serde, &decoders);

    let (outbound_done, inbound_done) =
        futures::future::try_join(outbound_running, inbound_running).await?;

    match (outbound_done, inbound_done) {}
}

async fn handle_outbound<Ctx, IO>(
    mut ctx: Ctx,
    output: IO,
    net_address: NetAddress,
    header_format: SerdeFormat,
    encoders: HashMap<TypeId, (usize, &dyn codecs::FormatSpecific)>,
) -> Result<Never, AnyError>
where
    Ctx: Messaging,
    IO: AsyncWrite,
{
    let output = pin!(output);
    let mut framed_write = FramedWrite::new(output, frame_codec());
    loop {
        let envelope = ctx.recv().await?;
        let tid = envelope.tid();
        let type_name = envelope.message_name();
        let to = envelope.header().to;
        let Some(ttl) = envelope.header().ttl.checked_sub(1) else {
            log::warn!("out of ttl [to: {}; message: {}]", to, type_name);
            continue
        };

        let Some((type_idx, enc)) = encoders.get(&tid) else {
            log::warn!(
                "attempt to send unsupported message [to: {}; net: {}; message: {}]",
                to,
                net_address,
                type_name
            );
            continue
        };

        let (any_message, _) = envelope.take();
        let Ok(encoded_message) = enc.encode(any_message).inspect_err(|e| {
            log::warn!(
                "could not encode message [message: {}; reason: {}]",
                type_name,
                e
            )
        }) else {
            continue
        };

        let header = Header {
            to,
            ttl,
            type_idx: *type_idx,
        };
        let packet = Packet::Envelope(header);
        let packet_bytes = packet.to_bytes(header_format)?;

        assert!(packet_bytes.len() <= MAX_FRAME_LEN);
        if encoded_message.len() > MAX_FRAME_LEN {
            log::warn!(
                "attempt to send a message larger than {} [to: {}; message: {}; len: {}]",
                MAX_FRAME_LEN,
                to,
                type_name,
                packet_bytes.len()
            );
            continue
        }

        framed_write.send(packet_bytes).await?;
        framed_write.send(encoded_message).await?;
    }
}

async fn handle_inbound<Ctx, IO>(
    ctx: &mut Ctx,
    input: IO,
    header_format: SerdeFormat,
    decoders: &[&dyn codecs::FormatSpecific],
) -> Result<Never, AnyError>
where
    Ctx: Messaging,
    IO: AsyncRead,
{
    let input = pin!(input);
    let mut framed_read = FramedRead::new(input, frame_codec());

    loop {
        let packet_bytes = framed_read
            .next()
            .await
            .transpose()?
            .ok_or("peer gone")?
            .freeze();
        let packet = Packet::from_bytes(header_format, packet_bytes)
            .inspect_err(|e| log::error!("could not parse packet: {}", e))?;
        match packet {
            Packet::Envelope(header) => {
                let Header { to, ttl, type_idx } = header;
                let Some(dec) = decoders.get(type_idx) else {
                    log::warn!(
                        "received envelope has bad type_idx [type_idx: {}; max: {}]",
                        type_idx,
                        decoders.len() - 1
                    );
                    continue
                };

                let encoded_message = framed_read
                    .next()
                    .await
                    .transpose()?
                    .ok_or("peer gone")?
                    .freeze();
                let Ok(any_message) = dec.decode(encoded_message).inspect_err(|reason| {
                    log::warn!(
                        "could not decode the received message [type_idx: {}; reason: {}]",
                        type_idx,
                        reason
                    )
                }) else {
                    continue
                };

                let header = EnvelopeHeader::to_address(to).with_ttl(ttl);
                let envelope = Envelope::new(header, any_message);

                let _ = ctx.send(envelope).await.inspect_err(|reason| {
                    log::warn!("could not send envelope [to: {}; reason: {}]", to, reason)
                });
            },
        }
    }
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
    Envelope(Header),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct Header {
    to:       Address,
    ttl:      usize,
    type_idx: usize,
}

impl Packet {
    fn from_bytes(serde_format: SerdeFormat, bytes: Bytes) -> Result<Self, AnyError> {
        match (serde_format, bytes) {
            #[cfg(feature = "format-json")]
            (SerdeFormat::Json, bytes) => serde_json::from_slice(&bytes).map_err(Into::into),
            #[cfg(feature = "format-bincode")]
            (SerdeFormat::Bincode, bytes) => {
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                    .map(|(packet, _)| packet)
                    .map_err(Into::into)
            },
            #[cfg(feature = "format-rmp")]
            (SerdeFormat::Rmp, bytes) => rmp_serde::decode::from_slice(&bytes).map_err(Into::into),
        }
    }

    fn to_bytes(&self, serde_format: SerdeFormat) -> Result<Bytes, AnyError> {
        match serde_format {
            #[cfg(feature = "format-json")]
            SerdeFormat::Json => serde_json::to_vec(self).map(Into::into).map_err(Into::into),
            #[cfg(feature = "format-bincode")]
            SerdeFormat::Bincode => {
                bincode::serde::encode_to_vec(self, bincode::config::standard())
                    .map(Into::into)
                    .map_err(Into::into)
            },
            #[cfg(feature = "format-rmp")]
            SerdeFormat::Rmp => {
                rmp_serde::encode::to_vec(self)
                    .map(Into::into)
                    .map_err(Into::into)
            },
        }
    }
}
