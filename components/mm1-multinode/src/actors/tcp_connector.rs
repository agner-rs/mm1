use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::log::{info, warn};
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::dispatch;
use mm1_proto::message;
use mm1_proto_network_management::protocols::ProtocolResolved;
use mm1_proto_network_management::{self as nm, protocols};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_system as sys;
use mm1_proto_well_known::MULTINODE_MANAGER;
use mm1_timer::v1::OneshotTimer;
use tokio::net::TcpStream;

use crate::actors::context::ActorContext;
use crate::codec::Protocol;

const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const PROTOCOL_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECTION_START_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    dst_addr: SocketAddr,
    protocol_name: nm::ProtocolName,
    _options: nm::Options,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let mut timer_api = OneshotTimer::create(ctx)
        .await
        .wrap_err("OneshotTimer::create")?;

    let protocol = wait_for_protocol(ctx, protocol_name)
        .await
        .wrap_err("wait_for_protocol")?;

    ctx.tell(ctx.address(), Connect)
        .await
        .wrap_err("ctx.tell")?;
    event_loop(ctx, &mut timer_api, connection_sup, dst_addr, protocol).await
}

async fn event_loop<Ctx>(
    ctx: &mut Ctx,
    timer_api: &mut OneshotTimer<Ctx>,
    connection_sup: Address,
    dst_addr: SocketAddr,
    protocol: ProtocolResolved<Protocol>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    let protocol = Arc::new(protocol);
    loop {
        let envelope = ctx.recv().await.wrap_err("ctx.recv")?;
        dispatch!(match envelope {
            Connect =>
                handle_connect(ctx, timer_api, connection_sup, dst_addr, protocol.clone())
                    .await
                    .wrap_err("handle_connect")?,

            sys::Down { normal_exit, .. } if *normal_exit => {
                info!("connection terminated normally [dst: {}]", dst_addr);
                break Ok(ctx.quit_ok().await)
            },
            sys::Down { normal_exit, .. } => {
                assert!(!normal_exit);

                warn!(
                    "connection terminated abnormally. Reconnecting in {:?} [dst: {}]",
                    RECONNECT_INTERVAL, dst_addr
                );
                timer_api
                    .schedule_once_after(RECONNECT_INTERVAL, Connect)
                    .await
                    .wrap_err("timer_api.schedule_once_after")?;
            },
        })
    }
}

async fn handle_connect<Ctx>(
    ctx: &mut Ctx,
    timer_api: &mut OneshotTimer<Ctx>,
    connection_sup: Address,
    dst_addr: SocketAddr,
    protocol: Arc<ProtocolResolved<Protocol>>,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    info!("connecting to {} from {:?}", dst_addr, dst_addr);

    let tcp_stream = match TcpStream::connect(dst_addr).await {
        Ok(tcp_stream) => tcp_stream,
        Err(reason) => {
            warn!("could not connect to {}: {}", dst_addr, reason);
            timer_api
                .schedule_once_after(RECONNECT_INTERVAL, Connect)
                .await
                .wrap_err("timer_api.schedule_once_after")?;
            return Ok(())
        },
    };

    let connection_addr = ctx
        .fork_ask::<_, uni_sup::StartResponse>(
            connection_sup,
            uni_sup::StartRequest {
                args: (tcp_stream, protocol),
            },
            CONNECTION_START_TIMEOUT,
        )
        .await
        .wrap_err("ctx.fork_ask")?
        .wrap_err("uni_sup::Start")?;

    let _watch_ref = ctx.watch(connection_addr).await;

    Ok(())
}

async fn wait_for_protocol<Ctx>(
    ctx: &mut Ctx,
    name: nm::ProtocolName,
) -> Result<ProtocolResolved<Protocol>, AnyError>
where
    Ctx: ActorContext,
{
    let resolved = ctx
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

    Ok(resolved)
}

#[message(base_path = ::mm1_proto)]
struct Connect;
