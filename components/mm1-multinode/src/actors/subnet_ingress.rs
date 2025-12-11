use std::any::TypeId;
use std::collections::HashMap;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::errors::chain::StdErrorDisplayChainExt;
use mm1_common::log::{debug, error, trace, warn};
use mm1_common::types::{AnyError, Never};
use mm1_core::context::BindArgs;
use mm1_core::envelope::{Envelope, dispatch};
use mm1_core::tracing::WithTraceIdExt;
use mm1_proto_network_management::protocols as p;

use crate::actors::context::ActorContext;
use crate::proto;
use crate::route_registry::RouteRegistry;

const INBOX_SIZE: usize = 1024;
const MULTINODE_MANAGER_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    net_address: NetAddress,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let mut subnet_ctx = ctx.fork().await.wrap_err("ctx.fork (subnet_ctx)")?;

    let () = subnet_ctx
        .bind(BindArgs {
            bind_to:    net_address,
            inbox_size: INBOX_SIZE,
        })
        .await
        .wrap_err("subnet_ctx.bind")?;

    let proto::SubscribeToRoutesResponse { routes } = ctx
        .ask(
            multinode_manager,
            proto::SubscribeToRoutesRequest {
                deliver_to: ctx.address(),
            },
            MULTINODE_MANAGER_TIMEOUT,
        )
        .await
        .wrap_err("ask (SubscribeToRoutes)")?;

    let mut route_registry = RouteRegistry::default();
    for proto::SetRoute {
        message,
        destination,
        via,
        metric,
    } in routes.into_iter().filter(|r| r.destination == net_address)
    {
        route_registry
            .set_route(message, destination, via, metric)
            .wrap_err("route_registry.set_route")?;
    }

    event_loop(
        ctx,
        &mut subnet_ctx,
        multinode_manager,
        net_address,
        &mut route_registry,
    )
    .await
    .wrap_err("event_loop")
}

async fn event_loop<Ctx>(
    ctx: &mut Ctx,
    subnet_ctx: &mut Ctx,
    multinode_manager: Address,
    net_address: NetAddress,
    route_registry: &mut RouteRegistry,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    let mut tid_cache = Default::default();
    loop {
        let worker_inbound = ctx.recv();
        let subnet_inbound = subnet_ctx.recv();

        tokio::select! {
            recv_result = worker_inbound => {
                let envelope = recv_result.wrap_err("worker_ctx.recv")?;
                let trace_id = envelope.header().trace_id();
                trace_id.scope_sync(||
                    process_worker_inbound(net_address, route_registry, envelope).wrap_err("process_worker_inbound"))?;
            },
            recv_result = subnet_inbound => {
                let envelope = recv_result.wrap_err("subnet_ctx.recv")?;
                let trace_id = envelope.header().trace_id();
                let () = process_subnet_inbound(ctx, multinode_manager, &mut tid_cache, route_registry, envelope).with_trace_id(trace_id).await.wrap_err("process_subnet_inbound")?;
            }
        }
    }
}

fn process_worker_inbound(
    net_address: NetAddress,
    route_registry: &mut RouteRegistry,
    envelope: Envelope,
) -> Result<(), AnyError> {
    dispatch!(match envelope {
        proto::SetRoute {
            message,
            destination,
            via,
            metric,
        } =>
            if destination == net_address {
                debug!(
                    msg = ?message, dst = %destination, ?via, ?metric,
                    "set route"
                );
                route_registry
                    .set_route(message, destination, via, metric)
                    .wrap_err("route_registry.set_route")?;
            },
        unexpected @ _ => {
            warn!(msg = ?unexpected, "unexpected message");
        },
    });
    Ok(())
}

async fn process_subnet_inbound<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    tid_cache: &mut HashMap<TypeId, p::LocalTypeKey>,
    route_registry: &RouteRegistry,
    envelope: Envelope,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    use std::collections::hash_map::Entry::*;
    // TODO: envelope.header().ttl

    let message_destination_address = envelope.header().to;

    let message_type_key = if let Some(proto::Forward { local_type_key, .. }) = envelope.peek() {
        *local_type_key
    } else {
        let type_id = envelope.tid();
        let message_type_key_opt = match tid_cache.entry(type_id) {
            Occupied(resolved) => Some(*resolved.get()),
            Vacant(new) => {
                let p::ResolveTypeIdResponse { type_key_opt } = ctx
                    .ask(
                        multinode_manager,
                        p::ResolveTypeIdRequest { type_id },
                        MULTINODE_MANAGER_TIMEOUT,
                    )
                    .await
                    .wrap_err("ask (ResolveTypeId)")?;
                if let Some(type_key) = type_key_opt {
                    new.insert(type_key);
                }
                type_key_opt
            },
        };

        let Some(message_type_key) = message_type_key_opt else {
            error!(
                dst = %message_destination_address, name = %envelope.message_name(),
                "no codec for the message"
            );
            return Ok(())
        };
        message_type_key
    };

    match route_registry.find_route(message_type_key, message_destination_address) {
        Err(reason) => {
            error!(
                msg = ?message_type_key, dst = %message_destination_address, reason = %reason.as_display_chain(),
                "can't find route"
            );
        },
        Ok((None, _)) => {
            error!("attempt to route a message via a subnet-ingress to a local-subnet");
        },
        Ok((Some(gw), _metric)) => {
            trace!(?gw, "forwarding message");
            if let Err(reason) = ctx.forward(gw, envelope).await {
                warn!(?gw, reason = %reason.as_display_chain(), "error forwarding a message");
            }
        },
    }

    Ok(())
}
