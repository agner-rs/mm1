use std::fmt;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log;
use mm1_core::context::{
    Fork, Linking, Quit, Recv, ShutdownErrorKind, Start, Stop, Tell, Watching,
};
use mm1_proto::{message, Message};
use mm1_proto_system::{StartErrorKind, System};

use crate::common::child_spec::{ChildSpec, InitType};

#[message]
pub(crate) struct Started<K> {
    pub child_id: K,
    pub address:  Address,
}

#[message]
pub(crate) struct StartFailed<K> {
    pub child_id: K,
}
#[message]
pub(crate) struct StopFailed {
    reason: ErrorOf<ShutdownErrorKind>,
}

pub(crate) async fn shutdown<Sys, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    address: Address,
    stop_timeout: Duration,
) where
    Ctx: Recv + Tell + Fork + Stop<Sys> + Watching<Sys>,
    Sys: System + Default,
{
    if let Err(reason) = ctx.shutdown(address, stop_timeout).await {
        send_report(ctx, sup_address, StopFailed { reason }).await;
    }
}

pub(crate) async fn run<K, Sys, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    child_id: K,
    child_spec: ChildSpec<Sys::Runnable>,
) where
    Sys: System,
    Ctx: Linking<Sys>,
    Ctx: Start<Sys>,
    Ctx: Quit + Tell,
    K: fmt::Display,
    Started<K>: Message,
    StartFailed<K>: Message,
{
    let ChildSpec {
        launcher: factory,
        init_type,
        child_type: _,
        stop_timeout: _,
    } = child_spec;
    let runnable = factory;

    match do_start(ctx, runnable, init_type).await {
        Ok(address) => {
            log::info!("{}[{}] started: {}", sup_address, child_id, address);
            send_report(ctx, sup_address, Started { child_id, address }).await;
        },
        Err(reason) => {
            log::info!("{}[{}] failed to start: {}", sup_address, child_id, reason);
            send_report(ctx, sup_address, StartFailed { child_id }).await;
        },
    };
}

async fn send_report<Ctx, M>(ctx: &mut Ctx, to: Address, report: M)
where
    Ctx: Tell,
    M: Message,
{
    ctx.tell(to, report).await.expect("failed to send report");
}

async fn do_start<Sys, Ctx>(
    ctx: &mut Ctx,
    runnable: Sys::Runnable,
    init_type: InitType,
) -> Result<Address, ErrorOf<StartErrorKind>>
where
    Sys: System,
    Ctx: Linking<Sys>,
    Ctx: Start<Sys>,
{
    match init_type {
        InitType::NoAck => {
            ctx.spawn(runnable, true)
                .await
                .map_err(|e| e.map_kind(StartErrorKind::Spawn))
        },
        InitType::WithAck { start_timeout } => ctx.start(runnable, true, start_timeout).await,
    }
}
