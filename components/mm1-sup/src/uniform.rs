use std::collections::HashSet;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_ask::Reply;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, warn};
use mm1_common::types::AnyError;
use mm1_core::context::{Fork, InitDone, Linking, Messaging, Quit, Start, Stop, Tell, Watching};
use mm1_core::envelope::dispatch;
use mm1_core::tracing::WithTraceIdExt;
use mm1_proto::message;
use mm1_proto_ask::{Request, RequestHeader};
use mm1_proto_sup::common as sup_common;
use mm1_proto_sup::uniform::{self as unisup};
use mm1_proto_system::{
    StartErrorKind, StopErrorKind, {self as system},
};

use crate::common::child_spec::{ChildSpec, InitType};
use crate::common::factory::ActorFactory;

pub trait UniformSupContext<Runnable>:
    Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}

pub struct UniformSup<F> {
    pub child_spec: ChildSpec<F, ()>,
}

impl<F> UniformSup<F> {
    pub fn new(child_spec: ChildSpec<F, ()>) -> Self {
        Self { child_spec }
    }
}

pub async fn uniform_sup<R, Ctx, F>(ctx: &mut Ctx, sup_spec: UniformSup<F>) -> Result<(), AnyError>
where
    R: Send + 'static,
    Ctx: UniformSupContext<R>,
    F: ActorFactory<Runnable = R>,
    F::Args: Send,
{
    let UniformSup { child_spec } = sup_spec;

    ctx.set_trap_exit(true).await;
    ctx.init_done(ctx.address()).await;

    let mut started_children: HashSet<Address> = Default::default();
    let mut stopping_children: HashSet<Address> = Default::default();

    loop {
        let envelope = ctx.recv().await.wrap_err("ctx.recv")?;
        let trace_id = envelope.header().trace_id();
        dispatch!(match envelope {
            Request::<_> {
                header: reply_to,
                payload: unisup::StartRequest::<F::Args> { args },
            } =>
                handle_start_request(ctx, &child_spec, reply_to, args)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_start_request")?,
            ChildStarted(child) => {
                ctx.link(child).await;
                assert!(started_children.insert(child));
            },

            Request::<_> {
                header: reply_to,
                payload: unisup::StopRequest { child },
            } =>
                handle_stop_request(ctx, &child_spec, &mut stopping_children, reply_to, child)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_stop_request")?,

            system::Exited { peer, normal_exit } =>
                handle_sys_exited(
                    ctx,
                    &mut started_children,
                    &mut stopping_children,
                    peer,
                    normal_exit
                )
                .with_trace_id(trace_id)
                .await,

            any @ _ => {
                trace_id.scope_sync(|| warn!("unexpected message: {:?}", any))
            },
        })
    }
}

async fn handle_sys_exited<Ctx>(
    ctx: &mut Ctx,
    started_children: &mut HashSet<Address>,
    stopping_children: &mut HashSet<Address>,
    peer: Address,
    normal_exit: bool,
) where
    Ctx: Quit,
{
    match (
        started_children.remove(&peer),
        stopping_children.remove(&peer),
        normal_exit,
    ) {
        (false, true, _) => unreachable!(),
        (true, true, normal_exit) => {
            debug!(
                "stopping child terminated [child: {}; normal_exit: {}]",
                peer, normal_exit
            )
        },
        (true, false, true) => {
            debug!(
                "running child normally terminated [child: {}; normal_exit: {}]",
                peer, normal_exit
            )
        },
        (true, false, false) => {
            warn!(
                "running child unexpectedly terminated [child: {}; normal_exit: {}]",
                peer, normal_exit
            )
        },
        (false, false, true) => (),
        (false, false, false) => {
            // TODO: reap all the children before giving up
            debug!(
                "unknown linked process terminated. Exitting. [offender: {}]",
                peer
            );
            ctx.quit_err(UnknownPeerExited(peer)).await;
        },
    }
}

async fn handle_start_request<Ctx, F, R>(
    ctx: &mut Ctx,

    child_spec: &ChildSpec<F, ()>,
    reply_to: RequestHeader,
    args: F::Args,
) -> Result<(), AnyError>
where
    R: Send + 'static,
    Ctx: UniformSupContext<R>,
    F: ActorFactory<Runnable = R>,
    F::Args: Send,
{
    debug!("start request [reply_to: {}]", reply_to);

    let sup_address = ctx.address();
    let ChildSpec {
        launcher: factory,
        init_type,
        announce_parent,
        child_type: (),
        stop_timeout: _,
    } = child_spec;
    let init_type = *init_type;
    let announce_parent = *announce_parent;

    let runnable = factory.produce(args);
    ctx.fork()
        .await
        .wrap_err("ctx.fork")?
        .run(move |mut ctx| {
            async move {
                let result =
                    do_start_child(&mut ctx, sup_address, init_type, announce_parent, runnable)
                        .await;
                ctx.reply(reply_to, result).await.ok();
            }
        })
        .await;
    Ok(())
}

async fn handle_stop_request<Ctx, F, R>(
    ctx: &mut Ctx,
    child_spec: &ChildSpec<F, ()>,
    stopping_children: &mut HashSet<Address>,
    reply_to: RequestHeader,
    child: Address,
) -> Result<(), AnyError>
where
    R: Send + 'static,
    Ctx: UniformSupContext<R>,
    F: ActorFactory<Runnable = R>,
    F::Args: Send,
{
    debug!("stop request [reply_to: {}; child: {}]", reply_to, child);

    let sup_address = ctx.address();
    let stop_timeout = child_spec.stop_timeout;

    if stopping_children.insert(child) {
        ctx.fork()
            .await
            .wrap_err("ctx.fork")?
            .run(move |mut ctx| {
                async move {
                    let result = do_stop_child(&mut ctx, sup_address, stop_timeout, child).await;
                    ctx.reply(reply_to, result).await.ok();
                }
            })
            .await;
    } else {
        ctx.reply(
            reply_to,
            unisup::StopResponse::Err(ErrorOf::new(StopErrorKind::NotFound, "not found")),
        )
        .await
        .ok();
    }
    Ok(())
}

async fn do_start_child<Runnable, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    init_type: InitType,
    announce_parent: bool,
    runnable: Runnable,
) -> unisup::StartResponse
where
    Ctx: Messaging + Start<Runnable>,
{
    debug!("starting child [init_type: {:?}]", init_type,);

    let result = match init_type {
        InitType::NoAck => {
            ctx.spawn(runnable, true)
                .await
                .map_err(|e| e.map_kind(StartErrorKind::Spawn))
        },
        InitType::WithAck { start_timeout } => ctx.start(runnable, true, start_timeout).await,
    };
    match result {
        Err(reason) => {
            warn!("error [reason: {}]", reason);
            Err(reason)
        },
        Ok(child) => {
            debug!("child [address: {}]", child);
            if announce_parent {
                debug!("child [address: {}] announcing parent", child);
                ctx.tell(
                    child,
                    sup_common::SetParent {
                        parent: sup_address,
                    },
                )
                .await
                .ok();
            }
            let _ = ctx.tell(sup_address, ChildStarted(child)).await;
            Ok(child)
        },
    }
}

async fn do_stop_child<Ctx>(
    ctx: &mut Ctx,
    _sup_address: Address,
    stop_timeout: Duration,
    child_address: Address,
) -> unisup::StopResponse
where
    Ctx: Fork + Stop + Watching + Messaging,
{
    debug!(
        "stopping child [child_address: {}, stop_timeout: {:?}]",
        child_address, stop_timeout
    );

    ctx.shutdown(child_address, stop_timeout)
        .await
        .map_err(|e| e.map_kind(|_| StopErrorKind::InternalError))
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
struct ChildStarted(Address);

#[derive(Debug, thiserror::Error)]
#[error("unknown peer failure: {}", _0)]
struct UnknownPeerExited(Address);

impl<F> Clone for UniformSup<F>
where
    ChildSpec<F, ()>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            child_spec: self.child_spec.clone(),
        }
    }
}

impl<Ctx, Runnable> UniformSupContext<Runnable> for Ctx where
    Ctx: Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}
