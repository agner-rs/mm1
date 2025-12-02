use std::collections::HashMap;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, warn};
use mm1_common::types::AnyError;
use mm1_core::context::{Fork, Linking, Messaging, Start, Stop, Tell, Watching};
use mm1_core::envelope::dispatch;
use mm1_core::tracing::WithTraceIdExt;
use mm1_proto::message;
use mm1_proto_ask::{Request, RequestHeader};
use mm1_proto_sup::common as sup_common;
use mm1_proto_sup::uniform::{self as unisup};
use mm1_proto_system::{
    StartErrorKind, StopErrorKind, {self as system},
};
use slotmap::SlotMap;

use crate::common::child_spec::{ChildSpec, InitType};
use crate::common::factory::ActorFactory;
use crate::uniform::child_type::UniformChildType;
use crate::uniform::{UniformSup, UniformSupContext};

pub async fn uniform_sup<R, Ctx, F, C>(
    ctx: &mut Ctx,
    sup_spec: UniformSup<F, C>,
) -> Result<(), AnyError>
where
    R: Send + 'static,
    Ctx: UniformSupContext<R>,
    F: ActorFactory<Runnable = R>,
    F::Args: Send,
    C: UniformChildType<F::Args>,
{
    let UniformSup { child_spec } = sup_spec;

    ctx.set_trap_exit(true).await;
    ctx.init_done(ctx.address()).await;

    let mut children: Children<C::Data> = Children {
        primary:    Default::default(),
        by_address: Default::default(),
    };

    loop {
        let envelope = ctx.recv().await.wrap_err("ctx.recv")?;
        let trace_id = envelope.header().trace_id();

        dispatch!(match envelope {
            Request::<_> {
                header: reply_to,
                payload: unisup::StartRequest::<F::Args> { args },
            } =>
                handle_start_request(ctx, &child_spec, &mut children, reply_to, args)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_start_request")?,

            ChildStarted { key, address } =>
                handle_child_started(ctx, &mut children, key, address)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_child_started")?,

            Request::<_> {
                header: reply_to,
                payload: unisup::StopRequest { child },
            } =>
                handle_stop_request(ctx, &child_spec, &mut children, reply_to, child)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_stop_request")?,

            system::Exited { peer, normal_exit } =>
                handle_sys_exited(ctx, &child_spec, &mut children, peer, normal_exit)
                    .with_trace_id(trace_id)
                    .await
                    .wrap_err("handle_sys_exited")?,

            unexpected @ _ => {
                trace_id.scope_sync(|| warn!("unexpected message: {:?}", unexpected));
            },
        });
    }
}

slotmap::new_key_type! {
    struct ChildKey;
}

#[derive(Debug)]
struct Children<D> {
    primary:    SlotMap<ChildKey, ChildEntry<D>>,
    by_address: HashMap<Address, ChildKey>,
}

#[derive(derive_more::Debug)]
struct ChildEntry<D> {
    status: ChildStatus,

    #[debug(skip)]
    data: D,
}

#[derive(Debug)]
enum ChildStatus {
    Starting,
    Started(Address),
    Stopping(Address),
}

async fn handle_start_request<Ctx, F, C>(
    ctx: &mut Ctx,
    child_spec: &ChildSpec<F, C>,
    children: &mut Children<C::Data>,
    reply_to: RequestHeader,
    args: F::Args,
) -> Result<(), AnyError>
where
    F: ActorFactory,
    Ctx: UniformSupContext<F::Runnable>,
    C: UniformChildType<F::Args>,
    F::Runnable: Send + 'static,
{
    let sup_address = ctx.address();
    let ChildSpec {
        launcher,
        child_type,
        init_type,
        stop_timeout: _,
        announce_parent,
    } = child_spec;
    let init_type = *init_type;
    let announce_parent = *announce_parent;

    let mut data = child_type.new_data(args);
    let runnable = child_type
        .make_runnable(launcher, &mut data)
        .wrap_err("child_type.make_runnable")?;
    let entry = ChildEntry {
        status: ChildStatus::Starting,
        data,
    };
    let key = children.primary.insert(entry);

    ctx.fork()
        .await
        .wrap_err("ctx.fork")?
        .run(async move |mut ctx| {
            let result = do_start_child(
                &mut ctx,
                sup_address,
                key,
                init_type,
                announce_parent,
                runnable,
            )
            .await;
            ctx.reply(reply_to, result).await.ok();
        })
        .await;

    debug!("child[{:?}] status Created -> Starting", key);

    Ok(())
}

async fn handle_child_started<Ctx, D>(
    ctx: &mut Ctx,
    children: &mut Children<D>,
    key: ChildKey,
    address: Address,
) -> Result<(), AnyError>
where
    Ctx: Linking,
{
    let Children {
        primary,
        by_address,
    } = children;
    let ChildEntry { status, data: _ } = &mut primary[key];
    if !matches!(*status, ChildStatus::Starting) {
        return Err(eyre::format_err!(
            "unexpected child-status when received child-started: {:?}",
            status
        ))
    }
    ctx.link(address).await;
    let should_be_none = by_address.insert(address, key);
    assert!(should_be_none.is_none());
    *status = ChildStatus::Started(address);

    debug!(
        "child[{:?}] status Starting -> Started [child-address: {}]",
        key, address
    );

    Ok(())
}

async fn handle_stop_request<Ctx, F, C>(
    ctx: &mut Ctx,
    child_spec: &ChildSpec<F, C>,
    children: &mut Children<C::Data>,
    reply_to: RequestHeader,
    address: Address,
) -> Result<(), AnyError>
where
    F: ActorFactory,
    Ctx: UniformSupContext<F::Runnable>,
    C: UniformChildType<F::Args>,
    F::Runnable: Send + 'static,
{
    let sup_address = ctx.address();
    let ChildSpec { stop_timeout, .. } = child_spec;
    let stop_timeout = *stop_timeout;

    let Children {
        primary,
        by_address,
    } = children;
    if let Some(key) = by_address.get(&address).copied() {
        let ChildEntry { status, data: _ } = &mut primary[key];
        match *status {
            ChildStatus::Starting => {
                unreachable!("how could we recover child of this state by address?")
            },
            ChildStatus::Started(a) => {
                assert_eq!(a, address);
            },
            ChildStatus::Stopping(a) => {
                assert_eq!(a, address);
                ctx.reply(
                    reply_to,
                    unisup::StopResponse::Err(ErrorOf::new(
                        StopErrorKind::NotFound,
                        format!("already stopping: {}", address),
                    )),
                )
                .await
                .ok();
                return Ok(())
            },
        };
        *status = ChildStatus::Stopping(address);

        ctx.fork()
            .await
            .wrap_err("ctx.fork")?
            .run(async move |mut ctx| {
                let reply_with = do_stop_child(&mut ctx, sup_address, stop_timeout, address).await;
                ctx.reply(reply_to, reply_with).await.ok();
            })
            .await;
    } else {
        ctx.reply(
            reply_to,
            unisup::StopResponse::Err(ErrorOf::new(
                StopErrorKind::NotFound,
                format!("unknown address: {}", address),
            )),
        )
        .await
        .ok();
    }

    Ok(())
}

async fn handle_sys_exited<Ctx, F, C>(
    ctx: &mut Ctx,
    child_spec: &ChildSpec<F, C>,
    children: &mut Children<C::Data>,
    peer: Address,
    normal_exit: bool,
) -> Result<(), AnyError>
where
    Ctx: UniformSupContext<F::Runnable>,
    F: ActorFactory,
    F::Args: Send,
    F::Runnable: Send + 'static,
    C: UniformChildType<F::Args>,
{
    let sup_address = ctx.address();
    let ChildSpec {
        launcher,
        child_type,
        init_type,
        stop_timeout: _,
        announce_parent,
    } = child_spec;

    let init_type = *init_type;
    let announce_parent = *announce_parent;

    let Children {
        primary,
        by_address,
    } = children;

    let Some(key) = by_address.remove(&peer) else {
        reap_started_children(ctx, child_spec, children)
            .await
            .wrap_err("reap children")?;
        ctx.quit_err(UnknownPeerExited(peer)).await;
        unreachable!()
    };

    let ChildEntry { status, data } = &mut primary[key];
    match *status {
        ChildStatus::Starting => {
            unreachable!("how could we recover child of this state by address?")
        },
        ChildStatus::Stopping(a) => {
            assert_eq!(a, peer);
            primary.remove(key);

            debug!(
                "child[{:?}] status Stopping -> Stopped [child-address: {}]",
                key, peer
            );

            Ok(())
        },
        ChildStatus::Started(a) => {
            if child_type.should_restart(data, normal_exit)? {
                assert_eq!(a, peer);

                let runnable = child_type
                    .make_runnable(launcher, data)
                    .wrap_err("child_type.make_runnable")?;

                *status = ChildStatus::Starting;

                debug!(
                    "child[{:?}] status Started -> Starting [child-address: {}]",
                    key, peer
                );

                ctx.fork()
                    .await
                    .wrap_err("ctx.fork")?
                    .run(async move |mut ctx| {
                        let _reply_with = do_start_child(
                            &mut ctx,
                            sup_address,
                            key,
                            init_type,
                            announce_parent,
                            runnable,
                        )
                        .await;
                    })
                    .await;

                Ok(())
            } else {
                primary.remove(key);

                debug!(
                    "child[{:?}] status Started -> Stopped (should not restart) [child-address: \
                     {}]",
                    key, peer
                );
                Ok(())
            }
        },
    }
}

async fn do_start_child<Runnable, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    child_key: ChildKey,
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
            let _ = ctx
                .tell(
                    sup_address,
                    ChildStarted {
                        key:     child_key,
                        address: child,
                    },
                )
                .await;
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

async fn reap_started_children<Ctx, F, C, D>(
    ctx: &mut Ctx,
    child_spec: &ChildSpec<F, C>,
    children: &mut Children<D>,
) -> Result<(), AnyError>
where
    Ctx: UniformSupContext<F::Runnable>,
    F: ActorFactory,
{
    let sup_address = ctx.address();
    let ChildSpec { stop_timeout, .. } = child_spec;
    let Children {
        primary,
        by_address,
    } = children;

    for (child_key, ChildEntry { status, data: _ }) in primary.drain() {
        let ChildStatus::Started(child_address) = status else {
            continue;
        };

        let should_be_child_key = by_address.remove(&child_address);
        assert_eq!(should_be_child_key, Some(child_key));

        do_stop_child(ctx, sup_address, *stop_timeout, child_address)
            .await
            .wrap_err("do_stop_child")?;
    }

    Ok(())
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
struct ChildStarted {
    key:     ChildKey,
    address: Address,
}

#[derive(Debug, thiserror::Error)]
#[error("unknown peer failure: {}", _0)]
struct UnknownPeerExited(Address);
