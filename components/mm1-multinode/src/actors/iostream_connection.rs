use std::any::TypeId;
use std::collections::{BTreeSet, HashMap};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_common::log::{debug, info, trace, warn};
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_proto::message;
use mm1_proto_network_management as nm;
use mm1_proto_network_management::protocols as p;
use mm1_timer::v1::OneshotTimer;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod config;
mod hello;
mod pdu;

use crate::actors::context::ActorContext;
use crate::codec::{ErasedCodec, Protocol};
use crate::proto::{self, SetRoute};
use crate::route_registry::RouteRegistry;

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run<Ctx, IO>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    mut io: IO,
    options: Arc<nm::Options>,
    protocol_resolved: Arc<p::ProtocolResolved<Protocol>>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
    IO: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    let conn_address = ctx.address();
    ctx.init_done(conn_address).await;

    let mut timer_api = OneshotTimer::create(ctx)
        .await
        .wrap_err("OneshotTimer::crate")?;

    let mut gw_ctx = ctx.fork().await.wrap_err("fork (gw_ctx)")?;
    let gw_address = gw_ctx.address();

    let io_reader_ctx = ctx.fork().await.wrap_err("fork (io_reader_ctx)")?;

    let p::ProtocolResolved {
        protocol,
        outbound,
        inbound,
    } = protocol_resolved.as_ref();

    let hello::HandshakeDone {} = hello::run(&mut io, &options).await.wrap_err("hello::run")?;

    let proto::SubscribeToRoutesResponse { routes } = ctx
        .fork_ask::<_, proto::SubscribeToRoutesResponse>(
            multinode_manager,
            proto::SubscribeToRoutesRequest {
                deliver_to: ctx.address(),
            },
            Duration::from_secs(5),
        )
        .await
        .wrap_err("ctx.fork_ask::<proto::SubscribeToRoutes>")?;

    let (io_reader, mut io_writer) = tokio::io::split(io);
    let inbound_by_name: HashMap<nm::MessageName, p::LocalTypeKey> =
        inbound.iter().cloned().collect();

    let inbound_by_lkey: HashMap<p::LocalTypeKey, ErasedCodec> = outbound
        .iter()
        .map(|(message_name, local_type_key)| {
            let codec = protocol
                .inbound_types()
                .find(|codec| codec.name() == *message_name)
                .expect("inbound-messages refers to the type that is not in the codec");
            (*local_type_key, codec.clone())
        })
        .collect();

    let outbound_by_type_id: HashMap<TypeId, (p::LocalTypeKey, ErasedCodec)> = outbound
        .iter()
        .map(|(message_name, local_type_key)| {
            let codec = protocol
                .outbound_types()
                .find(|codec| codec.name() == *message_name)
                .expect("outbound-messages refers to the type that is not in the codec");
            let tid = codec.tid().expect("protocol contains an opaque codec");
            (tid, (*local_type_key, codec.clone()))
        })
        .collect();

    io_reader_ctx
        .run(move |c| io_read_loop(c, io_reader, conn_address))
        .await;

    let () = declare_outbound_types(&mut io_writer, outbound.as_ref()).await?;

    let mut route_registry = RouteRegistry::default();
    let () = handle_set_routes(&mut io_writer, &mut route_registry, gw_address, &routes)
        .await
        .wrap_err("handle_set_routes (on init)")?;

    let local_subnets: p::GetLocalSubnetsResponse = ctx
        .fork_ask(
            multinode_manager,
            p::GetLocalSubnetsRequest,
            Duration::from_secs(1),
        )
        .await
        .wrap_err("ctx.fork_ask")?;

    event_loop(
        ctx,
        &mut gw_ctx,
        io_writer,
        &mut timer_api,
        &mut route_registry,
        multinode_manager,
        gw_address,
        &inbound_by_name,
        &inbound_by_lkey,
        &outbound_by_type_id,
        &local_subnets.into_iter().map(From::from).collect(),
    )
    .await
    .wrap_err("event_loop")
}

#[message(base_path= ::mm1_proto)]
struct KeepAliveTick;

#[allow(clippy::too_many_arguments)]
async fn event_loop<Ctx, W>(
    ctx: &mut Ctx,
    gw_ctx: &mut Ctx,
    mut io: W,
    timer_api: &mut OneshotTimer<Ctx>,
    route_registry: &mut RouteRegistry,
    multinode_manager: Address,
    gw: Address,
    inbound_by_name: &HashMap<nm::MessageName, p::LocalTypeKey>,
    inbound_by_lkey: &HashMap<p::LocalTypeKey, ErasedCodec>,
    outbound_by_type_id: &HashMap<TypeId, (p::LocalTypeKey, ErasedCodec)>,
    local_subnets: &BTreeSet<AddressRange>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
    W: AsyncWrite + Unpin,
{
    ctx.tell(ctx.address(), KeepAliveTick)
        .await
        .wrap_err("ctx.tell to self (KeepAliveTick)")?;

    let mut type_key_map = Default::default();
    loop {
        // let inbound = ctx.recv().await.wrap_err("ctx.recv")?;

        let to_connection = ctx.recv();
        let to_gw = gw_ctx.recv();

        tokio::select! {
            recv_result = to_connection => {
                let inbound = recv_result.wrap_err("ctx.recv")?;
                let () = handle_inbound(
                    ctx,
                    &mut io,
                    timer_api,
                    route_registry,
                    multinode_manager,
                    gw,
                    inbound_by_name,
                    inbound_by_lkey,
                    &mut type_key_map,
                    local_subnets,
                    inbound,
                )
                .await.wrap_err("handle_inbound")?;
            },
            recv_result = to_gw => {
                let to_forward = recv_result.wrap_err("ctx.recv")?;

                let () = handle_forward(ctx, &mut io, outbound_by_type_id, to_forward).await.wrap_err("handle_forward")?;
            }
        }
    }
}

async fn handle_forward<Ctx, W>(
    _ctx: &mut Ctx,
    mut io: W,
    outbound_by_type_id: &HashMap<TypeId, (p::LocalTypeKey, ErasedCodec)>,
    to_forward: Envelope,
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let dst_address = to_forward.header().to;
    // FIXME: other Header information is erased here

    let (message_type, body) = match to_forward.cast::<proto::Forward>() {
        Ok(to_forward) => {
            let (
                proto::Forward {
                    local_type_key,
                    body,
                },
                _,
            ) = to_forward.take();
            (local_type_key, body)
        },
        Err(to_forward) => {
            let tid = to_forward.tid();
            let &(message_type, ref codec) = outbound_by_type_id
                .get(&tid)
                .ok_or_else(|| eyre::format_err!("no codec for {}", to_forward.message_name()))?;

            let (message, _empty_envelope) = to_forward.take();

            let mut buf: Vec<u8> = vec![];
            codec.encode(&message, &mut buf).wrap_err("codec::encode")?;

            (message_type, buf.into_boxed_slice())
        },
    };

    let payload_size = body.len().try_into().wrap_err("message too large")?;

    let header = pdu::TransmitMessage {
        dst_address,
        message_type,
        payload_size,
    };

    trace!("writing header: {:?}", header);
    let () = util::write_header(&mut io, header)
        .await
        .wrap_err("util::write_header (TransmitMessage)")?;

    trace!("writing body [{} bytes]", body.len());
    let () = io.write_all(&body).await.wrap_err("write body")?;

    io.flush().await.wrap_err("io.flush")?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_inbound<Ctx, W>(
    ctx: &mut Ctx,
    io: W,
    timer_api: &mut OneshotTimer<Ctx>,
    route_registry: &mut RouteRegistry,
    multinode_manager: Address,
    gw: Address,
    inbound_by_name: &HashMap<nm::MessageName, p::LocalTypeKey>,
    inbound_by_lkey: &HashMap<p::LocalTypeKey, ErasedCodec>,
    type_key_map: &mut HashMap<p::ForeignTypeKey, p::LocalTypeKey>,
    local_subnets: &BTreeSet<AddressRange>,
    inbound: Envelope,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
    W: AsyncWrite + Unpin,
{
    use std::collections::hash_map::Entry::*;

    let mut io = pin!(io);

    use read_loop_proto as rl;
    dispatch!(match inbound {
        KeepAliveTick => {
            util::write_header(&mut io, pdu::Header::KeepAlive)
                .await
                .wrap_err("util::write_header")?;
            timer_api
                .schedule_once_after(KEEP_ALIVE_INTERVAL, KeepAliveTick)
                .await
                .wrap_err("timer_api.schedule_once_after")?;
            io.flush().await.wrap_err("io.flush")?;
        },

        rl::DeclareType {
            foreign_type_key,
            name,
        } => {
            let Vacant(entry) = type_key_map.entry(foreign_type_key) else {
                return Err(eyre::format_err!(
                    "duplicate type declaration [f-key: {:?}]",
                    foreign_type_key
                ))
            };
            if let Some(local_type_key) = inbound_by_name.get(&name).copied() {
                debug!(
                    "declared known type [f-key: {:?}; l-key: {:?}; name: {}]",
                    foreign_type_key, local_type_key, name
                );
                entry.insert(local_type_key);
            } else {
                let request = p::RegisterOpaqueMessageRequest { name: name.clone() };
                let p::RegisterOpaqueMessageResponse {
                    key: local_type_key,
                } = ctx
                    .fork_ask(multinode_manager, request, Duration::from_secs(1))
                    .await
                    .wrap_err("ctx.fork_ask")?;
                debug!(
                    "declared opaque type [f-key: {:?}; l-key: {:?}; name: {}]",
                    foreign_type_key, local_type_key, name
                );
                entry.insert(local_type_key);
            }
        },

        rl::SubnetDistance {
            net_address,
            type_handle,
            metric,
        } => {
            let local_type_key = type_key_map
                .get(&type_handle)
                .copied()
                .ok_or_else(|| eyre::format_err!("unregistered f-key: {:?}", type_handle))?;
            let request = proto::SetRoute {
                message:     local_type_key,
                destination: net_address,
                via:         Some(gw),
                metric:      metric.map(|m| m + 1 /* TODO: checked_add */),
            };
            ctx.tell(multinode_manager, request)
                .await
                .wrap_err("ctx.tell")?;
        },

        rl::ReceivedMessage {
            dst_address,
            foreign_type_key,
            body,
        } => {
            let local_type_key = *type_key_map
                .get(&foreign_type_key)
                .ok_or_else(|| eyre::format_err!("unregistered f-key: {:?}", foreign_type_key))?;

            if local_subnets.contains(&AddressRange::from(dst_address)) {
                let codec = inbound_by_lkey
                    .get(&local_type_key)
                    .ok_or_else(|| eyre::format_err!("no codec for l-key: {:?}", local_type_key))?;
                let any_message = codec.decode(&body).wrap_err("codec.decode")?;
                let header = EnvelopeHeader::to_address(dst_address);
                let to_deliver = Envelope::new(header, any_message);

                trace!("delivering [dst: {}]", dst_address);
                ctx.send(to_deliver).await.ok();
            } else {
                trace!(
                    "forwarding [dst: {}; l-key: {:?}]",
                    dst_address, local_type_key
                );
                let forward = proto::Forward {
                    local_type_key,
                    body,
                };
                ctx.tell(dst_address, forward).await.ok();
            }
        },

        set_route @ proto::SetRoute { .. } => {
            let () = handle_set_routes(&mut io, route_registry, gw, &[set_route])
                .await
                .wrap_err("handle_set_route")?;
        },

        unexpected @ _ => {
            warn!("UNEXPECTED MESSAGE: {:?}", unexpected);
        },
    });

    Ok(())
}

async fn handle_set_routes<W>(
    mut io: W,
    route_registry: &mut RouteRegistry,
    own_gw: Address,
    routes: &[SetRoute],
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    for set_route in routes.iter() {
        let SetRoute {
            message,
            destination,
            via,
            metric,
        } = set_route;
        let probe_address = AddressRange::from(*destination).lo();
        let metric_before = route_registry
            .find_route(*message, probe_address)
            .map(|(_, m)| m)
            .ok();
        route_registry
            .set_route(*message, *destination, *via, *metric)
            .wrap_err("set_route")?;
        let metric_after = route_registry
            .find_route(*message, probe_address)
            .map(|(_, m)| m)
            .ok();

        trace!(
            "handle_set_route [msg: {:?}, dst: {}; metric: {:?} -> {:?}; via: {:?}; own_gw: {:?}]",
            message, destination, metric_before, metric_after, via, own_gw
        );

        if metric_after != metric_before {
            let () = announce_route_to_peer(&mut io, set_route, own_gw)
                .await
                .wrap_err("write_own_route")?;
        }
    }
    Ok(())
}

async fn io_read_loop<Ctx, R>(mut ctx: Ctx, io: R, report_to: Address) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
    R: AsyncRead,
{
    use pdu::Header::*;
    use read_loop_proto as rl;

    let mut io = pin!(io);

    loop {
        let header = util::read_header(&mut io).await.wrap_err("read_header")?;
        match header {
            Hello(_unexpected_hello) => return Err(eyre::format_err!("unexpected hello")),

            KeepAlive => {},

            DeclareType(declare_type) => {
                let pdu::DeclareType {
                    message_type,
                    type_name_len,
                } = declare_type;
                let mut buf = vec![0u8; type_name_len as usize];
                io.read_exact(&mut buf[..]).await.wrap_err("read body")?;
                let type_name = String::from_utf8(buf).wrap_err("non UTF-8 name")?;
                info!(
                    "type declared [f-key: {:?}; name: {}]",
                    declare_type.message_type, type_name
                );

                let message = rl::DeclareType {
                    foreign_type_key: message_type,
                    name:             type_name.into(),
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },
            SubnetDistance(subnet_distance) => {
                let pdu::SubnetDistance {
                    net_address,
                    type_handle,
                    metric,
                } = subnet_distance;
                info!(
                    "foreign subnet [net: {}; f-key: {:?}; metric: {:?}]",
                    net_address, type_handle, metric
                );

                let message = rl::SubnetDistance {
                    net_address,
                    type_handle,
                    metric,
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },

            TransmitMessage(transmit_message) => {
                let pdu::TransmitMessage {
                    dst_address,
                    message_type,
                    payload_size,
                } = transmit_message;
                let mut buf = vec![0u8; payload_size as usize].into_boxed_slice();
                let _ = io
                    .read_exact(&mut buf[..])
                    .await
                    .wrap_err("io.read_exact (read body)")?;

                let message = rl::ReceivedMessage {
                    dst_address,
                    foreign_type_key: message_type,
                    body: buf,
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },
        }
    }
}

async fn declare_outbound_types<W>(
    io: W,
    outbound: &[(nm::MessageName, p::LocalTypeKey)],
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let mut io = pin!(io);
    for (type_name, message_type) in outbound {
        let type_name = type_name.as_bytes();
        let message_type = *message_type;
        let header = pdu::DeclareType {
            message_type,
            type_name_len: type_name.len().try_into().wrap_err("type-name too long")?,
        };
        util::write_header(&mut io, header)
            .await
            .wrap_err("write_header")?;
        io.write_all(type_name).await.wrap_err("write body")?;
        io.flush().await.wrap_err("io.flush")?;
    }
    Ok(())
}

async fn announce_route_to_peer<W>(
    mut io: W,
    set_route: &SetRoute,
    own_gw: Address,
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let SetRoute {
        message,
        destination,
        via,
        metric,
    } = set_route;

    if via.is_none_or(|gw| gw != own_gw) {
        trace!(
            "reporting to peer [msg: {:?}, dst: {}, metric: {:?}]",
            message, destination, metric
        );
        let header = pdu::SubnetDistance {
            net_address: *destination,
            type_handle: *message,
            metric:      *metric,
        };
        util::write_header(&mut io, header)
            .await
            .wrap_err("write_header")?;
        io.flush().await.wrap_err("io.flush")?;
    } else {
        trace!(
            "not reporting to peer the route going via own gw: {:?}",
            (message, destination, metric)
        );
    }
    Ok(())
}

mod read_loop_proto {
    use std::sync::Arc;

    use mm1_address::address::Address;
    use mm1_address::subnet::NetAddress;
    use mm1_proto::message;
    use mm1_proto_network_management::protocols::ForeignTypeKey;

    use crate::common::RouteMetric;

    #[message(base_path = ::mm1_proto)]
    pub(super) struct DeclareType {
        pub(super) foreign_type_key: ForeignTypeKey,
        pub(super) name:             Arc<str>,
    }

    #[message(base_path = ::mm1_proto)]
    pub(super) struct SubnetDistance {
        pub(super) net_address: NetAddress,
        pub(super) type_handle: ForeignTypeKey,
        pub(super) metric:      Option<RouteMetric>,
    }

    #[message(base_path = ::mm1_proto)]
    pub(super) struct ReceivedMessage {
        pub(super) dst_address:      Address,
        pub(super) foreign_type_key: ForeignTypeKey,
        pub(super) body:             Box<[u8]>,
    }
}

mod util {
    use std::pin::pin;

    use eyre::Context;
    use mm1_common::types::AnyError;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    use super::pdu::Header;
    use crate::actors::iostream_connection::pdu::{
        ForeignTypeKey, HEADER_FRAME_SIZE, LocalTypeKey,
    };

    pub(crate) async fn read_header<R>(io: R) -> Result<Header<ForeignTypeKey>, AnyError>
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

    pub(crate) async fn write_header<W>(
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
}
