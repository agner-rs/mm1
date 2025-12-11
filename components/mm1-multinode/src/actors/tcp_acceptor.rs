use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::log::{error, info};
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::{Envelope, dispatch};
use mm1_core::tracing::{TraceId, WithTraceIdExt};
use mm1_proto_network_management::protocols::ProtocolResolved;
use mm1_proto_network_management::{self as nm, protocols};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_well_known::MULTINODE_MANAGER;
use tokio::net::{TcpListener, TcpStream};

use crate::actors::context::ActorContext;
use crate::codec::Protocol;

const PROTOCOL_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_START_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    bind_addr: SocketAddr,
    protocol_names: Vec<nm::ProtocolName>,
    options: nm::Options,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let mut protocols = vec![];
    for protocol_name in protocol_names {
        let protocol = wait_for_protocol(ctx, protocol_name)
            .await
            .wrap_err("wait_for_protocol")?;
        protocols.push(protocol);
    }

    let tcp_listener = TcpListener::bind(bind_addr)
        .await
        .wrap_err("TcpListener::bind")?;

    event_loop(
        ctx,
        connection_sup,
        &tcp_listener,
        Arc::new(options),
        protocols.into_boxed_slice().into(),
    )
    .await
}

async fn wait_for_protocol<Ctx>(
    ctx: &mut Ctx,
    name: nm::ProtocolName,
) -> Result<protocols::ProtocolResolved<Protocol>, AnyError>
where
    Ctx: ActorContext,
{
    let found = ctx
        .ask::<_, protocols::GetProtocolByNameResponse<Protocol>>(
            MULTINODE_MANAGER,
            protocols::GetProtocolByNameRequest {
                name,
                timeout: Some(PROTOCOL_WAIT_TIMEOUT),
            },
            PROTOCOL_WAIT_TIMEOUT + Duration::from_secs(1),
        )
        .await
        .wrap_err("ask")?
        .wrap_err("GetProtocolByName")?;

    Ok(found)
}

async fn event_loop<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    tcp_listener: &TcpListener,
    options: Arc<nm::Options>,
    protocols: Arc<[ProtocolResolved<Protocol>]>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    loop {
        let accepted = tcp_listener.accept();
        let received = ctx.recv();

        tokio::select! {
            accept_result = accepted => {
                let (tcp_stream, peer_addr) = accept_result.wrap_err("tcp_listener.accept")?;
                let trace_id = TraceId::random();
                handle_accepted(ctx, connection_sup, options.clone(), protocols.clone(), tcp_stream, peer_addr).with_trace_id(trace_id).await.wrap_err("handle_accepted")?;
            },
            recv_result = received => {
                let envelope = recv_result.wrap_err("ctx.recv")?;
                let trace_id = envelope.header().trace_id();
                handle_envelope(ctx, envelope).with_trace_id(trace_id).await.wrap_err("handle_envelope")?;
            }
        }
    }
}

async fn handle_accepted<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    options: Arc<nm::Options>,
    protocols: Arc<[ProtocolResolved<Protocol>]>,
    tcp_stream: TcpStream,
    peer_addr: SocketAddr,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    let local_addr = tcp_stream.local_addr().wrap_err("tcp_stream.local_addr")?;
    info!(peer = %peer_addr, local = %local_addr, "accepted a connection");

    let connection_addr = ctx
        .ask::<_, uni_sup::StartResponse>(
            connection_sup,
            uni_sup::StartRequest {
                args: (tcp_stream, options, protocols),
            },
            CONNECTION_START_TIMEOUT,
        )
        .await
        .wrap_err("ask")?
        .wrap_err("uni_sup::Start")?;
    info!(
        %connection_addr,
        peer = %peer_addr, local = %local_addr,
        "connection started"
    );

    Ok(())
}

async fn handle_envelope<Ctx>(_ctx: &mut Ctx, envelope: Envelope) -> Result<(), AnyError> {
    dispatch!(match envelope {
        unexpected @ _ => error!(?unexpected, "received unexpected message"),
    });
    Ok(())
}
