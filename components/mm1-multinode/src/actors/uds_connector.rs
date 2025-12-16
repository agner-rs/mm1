use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::errors::chain::StdErrorDisplayChainExt;
use mm1_common::log::{info, warn};
use mm1_common::types::AnyError;
use mm1_proto::message;
use mm1_proto_network_management::protocols::ProtocolResolved;
use mm1_proto_network_management::{self as nm, protocols};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_system as sys;
use mm1_proto_well_known::MULTINODE_MANAGER;
use mm1_server::{OnMessage, Outcome};
use mm1_timer::v1::OneshotTimer;
use tokio::net::UnixStream;

use crate::actors::context::ActorContext;
use crate::codec::Protocol;

const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const PROTOCOL_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECTION_START_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    dst_addr: Box<Path>,
    protocol_names: Vec<nm::ProtocolName>,
    options: nm::Options,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let timer_api = OneshotTimer::create(ctx)
        .await
        .wrap_err("OneshotTimer::create")?;

    let mut protocols = vec![];
    for protocol_name in protocol_names {
        let protocol = wait_for_protocol(ctx, protocol_name)
            .await
            .wrap_err("wait_for_protocol")?;
        protocols.push(protocol);
    }

    ctx.tell(ctx.address(), Connect)
        .await
        .wrap_err("ctx.tell")?;

    mm1_server::new::<Ctx>()
        .behaviour(UdsConnector {
            timer_api,
            connection_sup,
            dst_addr,
            options: Arc::new(options),
            protocols: protocols.into_boxed_slice().into(),
        })
        .msg::<Connect>()
        .msg::<sys::Down>()
        .run(ctx)
        .await
        .wrap_err("server::run")?;

    Ok(())
}

struct UdsConnector<Ctx> {
    timer_api:      OneshotTimer<Ctx>,
    connection_sup: Address,
    dst_addr:       Box<Path>,
    options:        Arc<nm::Options>,
    protocols:      Arc<[ProtocolResolved<Protocol>]>,
}

impl<Ctx> OnMessage<Ctx, Connect> for UdsConnector<Ctx>
where
    Ctx: ActorContext,
{
    async fn on_message(
        &mut self,
        ctx: &mut Ctx,
        message: Connect,
    ) -> Result<Outcome<Connect>, AnyError> {
        let Connect = message;
        let Self {
            timer_api,
            connection_sup,
            dst_addr,
            options,
            protocols,
        } = self;
        let options = options.clone();
        let protocols = protocols.clone();

        info!("connecting to {:?} from {:?}", dst_addr, dst_addr);

        let uds_stream = match UnixStream::connect(&dst_addr).await {
            Ok(uds_stream) => uds_stream,
            Err(reason) => {
                warn!(dst = ?dst_addr, reason = %reason.as_display_chain(), "could not connect");
                timer_api
                    .schedule_once_after(RECONNECT_INTERVAL, Connect)
                    .await
                    .wrap_err("timer_api.schedule_once_after")?;
                return Ok(Outcome::no_reply())
            },
        };

        let connection_addr = ctx
            .ask::<_, uni_sup::StartResponse>(
                *connection_sup,
                uni_sup::StartRequest {
                    args: (uds_stream, options, protocols),
                },
                CONNECTION_START_TIMEOUT,
            )
            .await
            .wrap_err("ask")?
            .wrap_err("uni_sup::Start")?;

        let _watch_ref = ctx.watch(connection_addr).await;

        Ok(Outcome::no_reply())
    }
}

impl<Ctx> OnMessage<Ctx, sys::Down> for UdsConnector<Ctx>
where
    Ctx: ActorContext,
{
    async fn on_message(
        &mut self,
        _ctx: &mut Ctx,
        message: sys::Down,
    ) -> Result<Outcome<sys::Down>, AnyError> {
        let sys::Down { normal_exit, .. } = message;
        let Self {
            timer_api,
            dst_addr,
            ..
        } = self;

        if normal_exit {
            info!(dst = ?dst_addr, "connection terminated normally");
            Ok(Outcome::no_reply().then_stop())
        } else {
            warn!(
                dst = ?dst_addr,
                reconnecting_in = ?RECONNECT_INTERVAL,
                "connection terminated abnormally"
            );
            let _ = timer_api
                .schedule_once_after(RECONNECT_INTERVAL, Connect)
                .await
                .wrap_err("timer_api.schedule_once_after")?;

            Ok(Outcome::no_reply())
        }
    }
}

async fn wait_for_protocol<Ctx>(
    ctx: &mut Ctx,
    name: nm::ProtocolName,
) -> Result<ProtocolResolved<Protocol>, AnyError>
where
    Ctx: ActorContext,
{
    let resolved = ctx
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

    Ok(resolved)
}

#[message(base_path = ::mm1_proto)]
struct Connect;
