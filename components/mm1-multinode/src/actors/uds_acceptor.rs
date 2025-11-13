use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::log::{error, info};
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::{Envelope, dispatch};
use mm1_proto_network_management::protocols::ProtocolResolved;
use mm1_proto_network_management::{self as nm, protocols};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_well_known::MULTINODE_MANAGER;
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};

use crate::actors::context::ActorContext;
use crate::codec::Protocol;

const PROTOCOL_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_START_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    bind_addr: Box<Path>,
    protocol_name: nm::ProtocolName,
    _options: nm::Options,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let protocol = wait_for_protocol(ctx, protocol_name)
        .await
        .wrap_err("wait_for_protocol")?;
    let uds_listener = UnixListener::bind(bind_addr).wrap_err("UnixListener::bind")?;

    event_loop(ctx, connection_sup, &uds_listener, protocol).await
}

async fn wait_for_protocol<Ctx>(
    ctx: &mut Ctx,
    name: nm::ProtocolName,
) -> Result<protocols::ProtocolResolved<Protocol>, AnyError>
where
    Ctx: ActorContext,
{
    let found = ctx
        .fork_ask::<_, protocols::GetProtocolByNameResponse<Protocol>>(
            MULTINODE_MANAGER,
            protocols::GetProtocolByNameRequest {
                name,
                timeout: Some(PROTOCOL_WAIT_TIMEOUT),
            },
            PROTOCOL_WAIT_TIMEOUT + Duration::from_secs(1),
        )
        .await
        .wrap_err("fork_ask")?
        .wrap_err("GetProtocolByName")?;

    Ok(found)
}

async fn event_loop<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    uds_listener: &UnixListener,
    protocol: ProtocolResolved<Protocol>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    let protocol = Arc::new(protocol);
    loop {
        let accepted = uds_listener.accept();
        let received = ctx.recv();

        tokio::select! {
            accept_result = accepted => {
                let (uds_stream, peer_addr) = accept_result.wrap_err("uds_listener.accept")?;
                handle_accepted(ctx, connection_sup, protocol.clone(), uds_stream, peer_addr).await.wrap_err("handle_accepted")?;
            },
            recv_result = received => {
                let envelope = recv_result.wrap_err("ctx.recv")?;
                handle_envelope(ctx, envelope).await.wrap_err("handle_envelope")?;
            }
        }
    }
}

async fn handle_accepted<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    protocol: Arc<ProtocolResolved<Protocol>>,
    uds_stream: UnixStream,
    peer_addr: SocketAddr,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    let local_addr = uds_stream.local_addr().wrap_err("uds_stream.local_addr")?;
    info!(
        "accepted a connection form {:?} to {:?}",
        peer_addr, local_addr
    );

    let connection_addr = ctx
        .fork_ask::<_, uni_sup::StartResponse>(
            connection_sup,
            uni_sup::StartRequest {
                args: (uds_stream, protocol),
            },
            CONNECTION_START_TIMEOUT,
        )
        .await
        .wrap_err("fork_ask")?
        .wrap_err("uni_sup::Start")?;
    info!(
        "connection started {} [peer: {:?}; local: {:?}]",
        connection_addr, peer_addr, local_addr
    );

    Ok(())
}

async fn handle_envelope<Ctx>(_ctx: &mut Ctx, envelope: Envelope) -> Result<(), AnyError> {
    dispatch!(match envelope {
        unexpected @ _ => error!("received unexpected message: {:?}", unexpected),
    });
    Ok(())
}
