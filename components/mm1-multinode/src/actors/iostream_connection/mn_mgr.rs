use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::types::AnyError;
use mm1_proto_network_management as nm;
use mm1_proto_network_management::protocols::{self as p};

use crate::actors::context::ActorContext;
use crate::proto;

const ASK_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) async fn subscribe_to_routes<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    deliver_to: Address,
) -> Result<Vec<proto::SetRoute>, AnyError>
where
    Ctx: ActorContext,
{
    let proto::SubscribeToRoutesResponse { routes } = ctx
        .fork_ask::<_, proto::SubscribeToRoutesResponse>(
            multinode_manager,
            proto::SubscribeToRoutesRequest { deliver_to },
            ASK_TIMEOUT,
        )
        .await
        .wrap_err("ctx.fork_ask")?;

    Ok(routes)
}

pub(super) async fn get_local_subnets<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
) -> Result<Vec<NetAddress>, AnyError>
where
    Ctx: ActorContext,
{
    let local_subnets: p::GetLocalSubnetsResponse = ctx
        .fork_ask(multinode_manager, p::GetLocalSubnetsRequest, ASK_TIMEOUT)
        .await
        .wrap_err("ctx.fork_ask")?;

    Ok(local_subnets)
}

pub(super) async fn register_opaque_message<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    name: nm::MessageName,
) -> Result<p::LocalTypeKey, AnyError>
where
    Ctx: ActorContext,
{
    let request = p::RegisterOpaqueMessageRequest { name };
    let p::RegisterOpaqueMessageResponse { key } = ctx
        .fork_ask(multinode_manager, request, ASK_TIMEOUT)
        .await
        .wrap_err("ctx.fork_ask")?;

    Ok(key)
}

pub(super) async fn get_message_name<Ctx>(
    ctx: &mut Ctx,
    multinode_manager: Address,
    key: p::LocalTypeKey,
) -> Result<nm::MessageName, AnyError>
where
    Ctx: ActorContext,
{
    let request = p::GetMessageNameRequest { key };
    let p::GetMessageNameResponse { name } = ctx
        .fork_ask(multinode_manager, request, ASK_TIMEOUT)
        .await
        .wrap_err("ctx.fork_ask")?;
    Ok(name)
}
