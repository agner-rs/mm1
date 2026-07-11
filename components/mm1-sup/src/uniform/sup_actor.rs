use std::collections::HashMap;
use std::time::Duration;

use eyre::Context;
use mm1_address::address::Address;
use mm1_common::errors::chain::{ExactTypeDisplayChainExt, StdErrorDisplayChainExt};
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, warn};
use mm1_common::types::AnyError;
use mm1_core::context::{Fork, Linking, Messaging, Start, Stop, Tell, Watching};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_core::tracing::WithTraceIdExt;
use mm1_proto::{Message, message};
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

            ChildStartFailed { key } =>
                trace_id.scope_sync(|| handle_child_start_failed(&mut children, key)),

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
                trace_id.scope_sync(|| warn!(msg = ?unexpected, "unexpected message"));
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

    debug!(key = ?key, "child status Created -> Starting");

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
        key = ?key, address = %address,
        "child status Starting -> Started"
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
                key = ?key, peer = %peer,
                "child status Stopping -> Stopped"
            );

            Ok(())
        },
        ChildStatus::Started(a) => {
            let should_restart = match child_type.should_restart(data, normal_exit) {
                Ok(should_restart) => should_restart,
                Err(reason) => {
                    assert_eq!(a, peer);
                    primary.remove(key);

                    debug!(
                        key = ?key, peer = %peer,
                        "child status Started -> Stopped (restart decision failed)"
                    );

                    if let Err(cleanup_reason) =
                        reap_started_children(ctx, child_spec, children).await
                    {
                        warn!(
                            restart_reason = %reason.as_display_chain(),
                            cleanup_reason = %cleanup_reason.as_display_chain(),
                            "child cleanup failed after restart decision failed"
                        );
                    }
                    return Err(reason)
                },
            };

            if should_restart {
                assert_eq!(a, peer);

                let runnable = child_type
                    .make_runnable(launcher, data)
                    .wrap_err("child_type.make_runnable")?;

                *status = ChildStatus::Starting;

                debug!(
                    key = ?key, peer = %peer,
                    "child status Started -> Starting"
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
                    key = ?key, peer = %peer,
                    "child status Started -> Stopped (should not restart)"
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
    debug!(init_type = ?init_type, "starting child");

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
            warn!(reason = %reason.as_display_chain(), "error");
            // Tell the supervisor the start failed so it drops the leaked
            // `Starting` slot (#143a).
            notify_sup(ctx, sup_address, ChildStartFailed { key: child_key }).await;
            Err(reason)
        },
        Ok(child) => {
            debug!(address = %child, "child");
            if announce_parent {
                debug!(address = %child, "child announcing parent");
                ctx.tell(
                    child,
                    sup_common::SetParent {
                        parent: sup_address,
                    },
                )
                .await
                .ok();
            }
            notify_sup(
                ctx,
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
        child_address = %child_address, stop_timeout = ?stop_timeout,
        "stopping child"
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
    let mut first_error = None;

    for (child_key, ChildEntry { status, data: _ }) in primary.drain() {
        let ChildStatus::Started(child_address) = status else {
            continue;
        };

        let should_be_child_key = by_address.remove(&child_address);
        assert_eq!(should_be_child_key, Some(child_key));

        if let Err(reason) = do_stop_child(ctx, sup_address, *stop_timeout, child_address)
            .await
            .wrap_err("do_stop_child")
        {
            warn!(
                child_address = %child_address,
                reason = %reason.as_display_chain(),
                "could not stop child while reaping"
            );
            if first_error.is_none() {
                first_error = Some(reason);
            }
        }
    }

    match first_error {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// Send a supervision control message to the supervisor on the priority lane.
/// That lane is unbounded, so a full supervisor inbox cannot drop the message —
/// which otherwise leaves a started child unrecorded (#164) or leaks a failed
/// child's `Starting` slot (#143a). This runs in a fork of the supervisor.
async fn notify_sup<Ctx, M>(ctx: &mut Ctx, sup: Address, msg: M)
where
    Ctx: Messaging,
    M: Message,
{
    let envelope = Envelope::new(EnvelopeHeader::to_address(sup).with_priority(true), msg);
    if let Err(reason) = ctx.send(envelope.into_erased()).await {
        warn!(reason = %reason.as_display_chain(), "could not notify the supervisor");
    }
}

fn handle_child_start_failed<D>(children: &mut Children<D>, key: ChildKey) {
    // A child's start failed: drop its `Starting` slot so it is not leaked
    // forever (#143a).
    let is_starting = matches!(
        children.primary.get(key),
        Some(ChildEntry {
            status: ChildStatus::Starting,
            ..
        })
    );
    if is_starting {
        children.primary.remove(key);
        debug!(key = ?key, "child status Starting -> Stopped (start failed)");
    } else {
        warn!(key = ?key, "child-start-failed for an unexpected child state");
    }
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
struct ChildStarted {
    key:     ChildKey,
    address: Address,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
struct ChildStartFailed {
    key: ChildKey,
}

#[derive(Debug, thiserror::Error)]
#[error("unknown peer failure: {}", _0)]
struct UnknownPeerExited(Address);

#[cfg(test)]
mod tests {
    use mm1_address::pool::Pool;
    use mm1_address::subnet::NetMask;
    use mm1_core::context::ForkErrorKind;
    use mm1_test_rt::rt::event::{EventResolve, EventResolveResult};
    use mm1_test_rt::rt::{MainActorOutcome, TaskKey, TestContext, TestRuntime, query};

    use super::*;
    use crate::common::restart_intensity::RestartIntensity;

    async fn starter<Ctx>(ctx: &mut Ctx, sup: Address)
    where
        Ctx: Messaging + Start<()>,
    {
        let _ = do_start_child(ctx, sup, ChildKey::default(), InitType::NoAck, false, ()).await;
    }

    /// #164: the `ChildStarted` report a started child sends back to the uniform
    /// supervisor must go on the priority lane, so a full supervisor inbox
    /// cannot drop it (which would strand the child, unrecorded).
    #[tokio::test]
    async fn child_started_report_uses_the_priority_lane() {
        let rt = TestRuntime::<()>::new();
        let subnet = Pool::new("<ff:>/16".parse().unwrap());
        let starter_lease = subnet.lease(NetMask::M_32).unwrap();
        let sup_lease = subnet.lease(NetMask::M_32).unwrap();
        let child_lease = subnet.lease(NetMask::M_32).unwrap();
        let starter_addr = starter_lease.address;
        let sup_addr = sup_lease.address;
        let child_addr = child_lease.address;

        rt.add_actor(starter_addr, Some(starter_lease), (starter, (sup_addr,)))
            .await
            .unwrap();

        // do_start_child spawns the child (NoAck) ...
        let spawn = rt
            .next_event()
            .await
            .unwrap()
            .unwrap()
            .convert::<query::Spawn<()>>()
            .unwrap();
        spawn.resolve_ok(child_addr);

        // ... then reports `ChildStarted` back to the supervisor.
        let tell = rt
            .next_event()
            .await
            .unwrap()
            .unwrap()
            .convert::<query::Tell>()
            .unwrap();
        assert_eq!(tell.to, sup_addr);
        assert!(
            tell.envelope.header().priority,
            "#164: the ChildStarted report must be on the priority lane"
        );
    }

    #[derive(Debug)]
    struct UnitFactory;

    impl ActorFactory for UnitFactory {
        type Args = ();
        type Runnable = ();

        fn produce(&self, (): Self::Args) -> Self::Runnable {}
    }

    async fn fail_restart_intensity(
        ctx: &mut TestContext<()>,
        failed_child: Address,
        surviving_child: Address,
    ) -> Result<(), AnyError> {
        let child_type = crate::uniform::child_type::Permanent::new(RestartIntensity {
            max_restarts: 0,
            within:       Duration::MAX,
        });
        let mut children = Children {
            primary:    Default::default(),
            by_address: Default::default(),
        };

        for child_address in [failed_child, surviving_child] {
            let key = children.primary.insert(ChildEntry {
                status: ChildStatus::Started(child_address),
                data:   child_type.new_data(()),
            });
            assert_eq!(children.by_address.insert(child_address, key), None);
        }

        let child_spec = ChildSpec::new(UnitFactory)
            .with_child_type(child_type)
            .with_stop_timeout(Duration::from_secs(10));

        handle_sys_exited(ctx, &child_spec, &mut children, failed_child, false).await
    }

    /// #164: a uniform supervisor that reaches a child's restart-intensity
    /// limit must gracefully reap its other started children before returning
    /// the intensity error.
    #[tokio::test]
    async fn restart_intensity_failure_reaps_started_siblings_before_returning() {
        tokio::time::pause();

        let subnet = Pool::new("<ff:>/16".parse().unwrap());
        let sup_lease = subnet.lease(NetMask::M_32).unwrap();
        let failed_child_lease = subnet.lease(NetMask::M_32).unwrap();
        let surviving_child_lease = subnet.lease(NetMask::M_32).unwrap();
        let fork_lease = subnet.lease(NetMask::M_32).unwrap();
        let sup_address = sup_lease.address;
        let failed_child = failed_child_lease.address;
        let surviving_child = surviving_child_lease.address;
        let fork_address = fork_lease.address;

        let rt = TestRuntime::<()>::new();
        rt.add_actor(
            sup_address,
            Some(sup_lease),
            (fail_restart_intensity, (failed_child, surviving_child)),
        )
        .await
        .unwrap();

        // Reaping starts by reserving a context for the shutdown sequence. On
        // the buggy path the actor is already Done here because the intensity
        // error escaped before any attempt to stop the surviving child.
        let fork = rt
            .expect_next_event()
            .await
            .convert::<query::Fork<()>>()
            .expect("restart-intensity failure must start reaping survivors");
        assert_eq!(fork.task_key, TaskKey::actor(sup_address));
        fork.resolve_ok(rt.new_context(TaskKey::fork(sup_address, fork_address), Some(fork_lease)));

        let watch = rt
            .expect_next_event()
            .await
            .convert::<query::Watch>()
            .unwrap();
        assert_eq!(watch.task_key, TaskKey::fork(sup_address, fork_address));
        assert_eq!(watch.peer, surviving_child);
        let watch_ref = system::WatchRef::MIN;
        watch.resolve(watch_ref);

        // `shutdown` polls its exit request and its Down receiver together, so
        // either query may reach the test runtime first. Keep the receive query
        // pending until the graceful Exit has been observed and accepted.
        let event = rt.expect_next_event().await;
        let recv = match event.convert::<query::Exit>() {
            Ok(exit) => {
                assert_eq!(exit.task_key, TaskKey::actor(sup_address));
                assert_eq!(exit.peer, surviving_child);
                exit.resolve(true);
                rt.expect_next_event()
                    .await
                    .convert::<query::Recv>()
                    .unwrap()
            },
            Err(event) => {
                let recv = event.convert::<query::Recv>().unwrap();
                let exit = rt
                    .expect_next_event()
                    .await
                    .convert::<query::Exit>()
                    .unwrap();
                assert_eq!(exit.task_key, TaskKey::actor(sup_address));
                assert_eq!(exit.peer, surviving_child);
                exit.resolve(true);
                recv
            },
        };
        assert_eq!(recv.task_key, TaskKey::fork(sup_address, fork_address));
        recv.resolve_ok(
            Envelope::new(
                EnvelopeHeader::to_address(fork_address),
                system::Down {
                    peer: surviving_child,
                    watch_ref,
                    normal_exit: true,
                },
            )
            .into_erased(),
        );

        // The restart-intensity error is applied as ActorExit only after the
        // survivor has completed its shutdown sequence.
        let quit = rt
            .expect_next_event()
            .await
            .convert::<query::Quit>()
            .unwrap();
        let reason = quit.result.as_ref().unwrap_err().to_string();
        assert!(reason.contains("max restart intensity reached"), "{reason}");
        quit.stop_tasks().await.unwrap();

        let done = rt
            .expect_next_event()
            .await
            .convert::<MainActorOutcome>()
            .unwrap();
        assert_eq!(done.address, sup_address);
        done.remove_actor_entry().await.unwrap();
    }

    async fn fail_restart_intensity_and_report_result(
        ctx: &mut TestContext<()>,
        failed_child: Address,
        surviving_child_a: Address,
        surviving_child_b: Address,
        result_tx: tokio::sync::oneshot::Sender<String>,
    ) -> Result<(), AnyError> {
        let child_type = crate::uniform::child_type::Permanent::new(RestartIntensity {
            max_restarts: 0,
            within:       Duration::MAX,
        });
        let mut children = Children {
            primary:    Default::default(),
            by_address: Default::default(),
        };

        for child_address in [failed_child, surviving_child_a, surviving_child_b] {
            let key = children.primary.insert(ChildEntry {
                status: ChildStatus::Started(child_address),
                data:   child_type.new_data(()),
            });
            assert_eq!(children.by_address.insert(child_address, key), None);
        }

        let child_spec = ChildSpec::new(UnitFactory)
            .with_child_type(child_type)
            .with_stop_timeout(Duration::from_secs(10));

        let result = handle_sys_exited(ctx, &child_spec, &mut children, failed_child, false).await;
        let reason = result
            .as_ref()
            .expect_err("restart intensity must fail")
            .to_string();
        result_tx
            .send(reason)
            .expect("test must receive the restart-intensity result");
        result
    }

    #[tokio::test]
    async fn cleanup_failure_does_not_hide_intensity_error_or_skip_later_siblings() {
        let subnet = Pool::new("<ff:>/16".parse().unwrap());
        let sup_lease = subnet.lease(NetMask::M_32).unwrap();
        let failed_child_lease = subnet.lease(NetMask::M_32).unwrap();
        let surviving_child_a_lease = subnet.lease(NetMask::M_32).unwrap();
        let surviving_child_b_lease = subnet.lease(NetMask::M_32).unwrap();
        let sup_address = sup_lease.address;
        let failed_child = failed_child_lease.address;
        let surviving_child_a = surviving_child_a_lease.address;
        let surviving_child_b = surviving_child_b_lease.address;
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let rt = TestRuntime::<()>::new();
        rt.add_actor(
            sup_address,
            Some(sup_lease),
            (
                fail_restart_intensity_and_report_result,
                (
                    failed_child,
                    surviving_child_a,
                    surviving_child_b,
                    result_tx,
                ),
            ),
        )
        .await
        .unwrap();

        let first_fork = rt
            .expect_next_event()
            .await
            .convert::<query::Fork<()>>()
            .expect("the first survivor must enter shutdown");
        assert_eq!(first_fork.task_key, TaskKey::actor(sup_address));
        first_fork.resolve_err(ErrorOf::new(
            ForkErrorKind::ResourceConstraint,
            "injected first cleanup failure",
        ));

        // The first cleanup failed, but the later survivor must still get its
        // own shutdown attempt.
        let second_fork = rt
            .expect_next_event()
            .await
            .convert::<query::Fork<()>>()
            .expect("cleanup failure must not skip the later survivor");
        assert_eq!(second_fork.task_key, TaskKey::actor(sup_address));
        second_fork.resolve_err(ErrorOf::new(
            ForkErrorKind::ResourceConstraint,
            "injected second cleanup failure",
        ));

        let quit = rt
            .expect_next_event()
            .await
            .convert::<query::Quit>()
            .unwrap();
        let reason = result_rx.await.unwrap();
        assert_eq!(reason, "max restart intensity reached");

        let reason = quit.result.as_ref().unwrap_err().to_string();
        assert!(reason.contains("max restart intensity reached"), "{reason}");
        quit.stop_tasks().await.unwrap();

        let done = rt
            .expect_next_event()
            .await
            .convert::<MainActorOutcome>()
            .unwrap();
        assert_eq!(done.address, sup_address);
        done.remove_actor_entry().await.unwrap();
    }
}
