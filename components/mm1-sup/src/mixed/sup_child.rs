use std::fmt;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log;
use mm1_core::context::{
    Fork, Linking, Messaging, Quit, ShutdownErrorKind, Start, Stop, Tell, Watching,
};
use mm1_proto::{Message, message};
use mm1_proto_sup::common as sup_common;
use mm1_proto_system::StartErrorKind;

use crate::common::child_spec::{ChildSpec, InitType};

#[message(base_path = ::mm1_proto)]
pub(crate) struct Started<K> {
    pub child_id: K,
    pub address:  Address,
}

#[message(base_path = ::mm1_proto)]
pub(crate) struct StartFailed<K> {
    pub child_id: K,
}
#[message(base_path = ::mm1_proto)]
pub(crate) struct StopFailed {
    pub address: Address,
    pub reason:  ErrorOf<ShutdownErrorKind>,
}

pub(crate) async fn shutdown<Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    address: Address,
    stop_timeout: Duration,
) where
    Ctx: Messaging + Fork + Stop + Watching,
{
    if let Err(reason) = ctx.shutdown(address, stop_timeout).await {
        send_report(ctx, sup_address, StopFailed { address, reason }).await;
    }
}

pub(crate) async fn run<K, Runnable, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    child_id: K,
    child_spec: ChildSpec<Runnable>,
) where
    Ctx: Linking + Start<Runnable> + Quit + Messaging,
    K: fmt::Display,
    Started<K>: Message,
    StartFailed<K>: Message,
{
    let ChildSpec {
        launcher: factory,
        init_type,
        child_type: _,
        stop_timeout: _,
        announce_parent,
    } = child_spec;
    let runnable = factory;

    match do_start(ctx, runnable, init_type).await {
        Ok(address) => {
            log::info!("{}[{}] started: {}", sup_address, child_id, address);
            if announce_parent {
                log::debug!("{}[{}] announcing parent", sup_address, child_id);
                ctx.tell(
                    address,
                    sup_common::SetParent {
                        parent: sup_address,
                    },
                )
                .await
                .ok();
            }
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
    Ctx: Messaging,
    M: Message,
{
    ctx.tell(to, report).await.expect("failed to send report");
}

async fn do_start<Runnable, Ctx>(
    ctx: &mut Ctx,
    runnable: Runnable,
    init_type: InitType,
) -> Result<Address, ErrorOf<StartErrorKind>>
where
    Ctx: Linking + Start<Runnable>,
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
