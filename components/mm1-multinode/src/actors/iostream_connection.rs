use std::any::TypeId;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_common::log::{debug, info, trace, warn};
use mm1_common::make_metrics;
use mm1_common::metrics::MeasuredFutureExt;
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_core::tracing::WithTraceIdExt;
use mm1_proto::message;
use mm1_proto_network_management as nm;
use mm1_proto_network_management::protocols as p;
use mm1_timer::v1::OneshotTimer;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tracing::{Instrument, Level};

mod config;
mod hello;
mod iostream_read_loop;
mod iostream_util;
mod iostream_write;
mod mn_mgr;
mod multiple_protocols;
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
    protocols_resolved: Arc<[p::ProtocolResolved<Protocol>]>,
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
    } = multiple_protocols::reduce(&protocols_resolved).wrap_err("multiple_protocols::reduce")?;

    let hello::HandshakeDone {} = hello::run(&mut io, &options).await.wrap_err("hello::run")?;

    let routes = mn_mgr::subscribe_to_routes(ctx, multinode_manager, ctx.address())
        .await
        .wrap_err("mn_mgr::subscribe_to_routes")?;

    let (io_reader, io_writer) = tokio::io::split(io);

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

    let mut output_writer = iostream_write::OutputWriter::new(
        ctx.fork().await.wrap_err("ctx.fork (for WriteContext)")?,
        io_writer,
        multinode_manager,
        outbound_by_type_id,
    );

    io_reader_ctx
        .run(move |c| iostream_read_loop::run(c, io_reader, conn_address))
        .await;

    for (name, key) in outbound.iter() {
        output_writer
            .write_delcare_type(*key, name.clone())
            .await
            .wrap_err("output_writer.write_declare_type")?;
    }

    let mut route_registry = RouteRegistry::default();
    let () = handle_set_routes(&mut output_writer, &mut route_registry, gw_address, &routes)
        .await
        .wrap_err("handle_set_routes (on init)")?;

    let local_subnets = mn_mgr::get_local_subnets(ctx, multinode_manager)
        .await
        .wrap_err("mn_gr::get_local_subnets")?;

    event_loop(
        ctx,
        &mut gw_ctx,
        &mut output_writer,
        &mut timer_api,
        &mut route_registry,
        multinode_manager,
        gw_address,
        &inbound_by_name,
        &inbound_by_lkey,
        &local_subnets.into_iter().map(From::from).collect(),
    )
    .await
    .wrap_err("event_loop")
}

#[message(base_path = ::mm1_proto)]
struct KeepAliveTick;

#[allow(clippy::too_many_arguments)]
async fn event_loop<Ctx, W>(
    ctx: &mut Ctx,
    gw_ctx: &mut Ctx,
    output_writer: &mut iostream_write::OutputWriter<Ctx, W>,
    timer_api: &mut OneshotTimer<Ctx>,
    route_registry: &mut RouteRegistry,
    multinode_manager: Address,
    gw: Address,
    inbound_by_name: &HashMap<nm::MessageName, p::LocalTypeKey>,
    inbound_by_lkey: &HashMap<p::LocalTypeKey, ErasedCodec>,
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
    let mut unique_message_names = HashSet::<Arc<str>>::new();

    loop {
        let to_connection = ctx.recv();
        let to_gw = gw_ctx.recv();

        tokio::select! {
            recv_result = to_connection => {
                let inbound = recv_result.wrap_err("ctx.recv")?;
                let message_name = inbound.message_name();
                let message_name =
                    if let Some(existing) = unique_message_names.get(message_name) {
                        existing
                    } else {
                        unique_message_names.insert(message_name.into());
                        unique_message_names.get(message_name).unwrap()
                    };
                let span = tracing::span!(Level::TRACE, "mm1_multinode_iostream_on_message",
                    io = std::any::type_name::<W>(),
                    msg = %message_name,
                );
                let metrics = make_metrics!("mm1_multinode_iostream_on_message",
                    "io" => std::any::type_name::<W>(),
                    "msg" => message_name.clone(),
                );

                let trace_id = inbound.header().trace_id();
                let () = handle_connection_actor_message(
                    ctx,
                    output_writer,
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
                .measured(metrics)
                .instrument(span)
                .with_trace_id(trace_id)
                .await.wrap_err("handle_inbound")?;
            },
            recv_result = to_gw => {
                let to_forward = recv_result.wrap_err("ctx.recv")?;
                let trace_id = to_forward.header().trace_id();
                let message_name = to_forward.message_name();
                let message_name =
                    if let Some(existing) = unique_message_names.get(message_name) {
                        existing
                    } else {
                        unique_message_names.insert(message_name.into());
                        unique_message_names.get(message_name).unwrap()
                    };

                let span = tracing::span!(Level::TRACE, "mm1_multinode_iostream_on_forward",
                    io = std::any::type_name::<W>(),
                    msg = %message_name,
                );
                let metrics = make_metrics!("mm1_multinode_iostream_on_forward",
                    "io" => std::any::type_name::<W>(),
                    "msg" => message_name.clone(),
                );

                let () = handle_forward_actor_message(ctx, output_writer, to_forward)
                    .measured(metrics)
                    .instrument(span)
                    .with_trace_id(trace_id)
                    .await.wrap_err("handle_forward")?;
            }
        }
    }
}

async fn handle_forward_actor_message<Ctx, W>(
    _ctx: &mut Ctx,
    output_writer: &mut iostream_write::OutputWriter<Ctx, W>,
    to_forward: Envelope,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
    W: AsyncWrite + Unpin,
{
    let (message, empty_envelope) = to_forward.take();
    let envelope_header = empty_envelope.header();

    match message.cast::<proto::Forward>() {
        Ok(to_forward) => {
            output_writer
                .write_opaque_message(envelope_header, to_forward)
                .await
                .wrap_err("output_writer.write_opaque_message")?
        },
        Err(to_forward) => {
            output_writer
                .write_known_message(envelope_header, to_forward)
                .await
                .wrap_err("output_writer.write_known_message")?;
        },
    };

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection_actor_message<Ctx, W>(
    ctx: &mut Ctx,
    output_writer: &mut iostream_write::OutputWriter<Ctx, W>,
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

    use iostream_read_loop as rl;
    dispatch!(match inbound {
        KeepAliveTick => {
            output_writer
                .write_keep_alive()
                .await
                .wrap_err("output_writer.write_keep_alive")?;
            timer_api
                .schedule_once_after(KEEP_ALIVE_INTERVAL, KeepAliveTick)
                .await
                .wrap_err("timer_api.schedule_once_after")?;
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
                    f_key = ?foreign_type_key, l_key = ?local_type_key, %name,
                    "declared known type"
                );
                entry.insert(local_type_key);
            } else {
                let local_type_key =
                    mn_mgr::register_opaque_message(ctx, multinode_manager, name.clone())
                        .await
                        .wrap_err("mn_mgr::register_opaque_message")?;

                debug!(
                    f_key = ?foreign_type_key, l_key = ?local_type_key, %name,
                    "declared opaque type"
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
            trace_id,
            origin_seq_no,
            ttl,
            priority,
            foreign_type_key,
            body,
        } => {
            let local_type_key = *type_key_map
                .get(&foreign_type_key)
                .ok_or_else(|| eyre::format_err!("unregistered f-key: {:?}", foreign_type_key))?;

            let header = EnvelopeHeader::to_address(dst_address)
                .with_trace_id(trace_id)
                .with_no(origin_seq_no)
                .with_ttl(ttl)
                .with_priority(priority);

            if local_subnets.contains(&AddressRange::from(dst_address)) {
                let codec = inbound_by_lkey
                    .get(&local_type_key)
                    .ok_or_else(|| eyre::format_err!("no codec for l-key: {:?}", local_type_key))?;
                let any_message = codec.decode(&body).wrap_err("codec.decode")?;
                let to_deliver = Envelope::new(header, any_message);

                trace!(dst = %dst_address, "delivering");
                ctx.send(to_deliver).await.ok();
            } else {
                trace!(
                    dst = %dst_address, l_key = ?local_type_key,
                    "forwarding"
                );
                let forward = proto::Forward {
                    local_type_key,
                    body,
                };
                let envelope = Envelope::new(header, forward).into_erased();
                ctx.send(envelope).await.ok();
            }
        },

        set_route @ proto::SetRoute { .. } => {
            let () = handle_set_routes(output_writer, route_registry, gw, &[set_route])
                .await
                .wrap_err("handle_set_route")?;
        },

        unexpected @ _ => {
            warn!(msg = ?unexpected, "UNEXPECTED MESSAGE");
        },
    });

    Ok(())
}

async fn handle_set_routes<Ctx, W>(
    output_writer: &mut iostream_write::OutputWriter<Ctx, W>,
    route_registry: &mut RouteRegistry,
    own_gw: Address,
    routes: &[SetRoute],
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
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
            ?message, dst = %destination, ?metric, ?via, ?own_gw,
            "handle_set_route"
        );

        if metric_after != metric_before && via.is_none_or(|gw| gw != own_gw) {
            trace!(
                ?message, dst = %destination, ?metric,
                "reporting to peer"
            );
            output_writer
                .write_subnet_distance(set_route)
                .await
                .wrap_err("output_writer.write_subnet_distance")?;
        }
    }
    Ok(())
}
