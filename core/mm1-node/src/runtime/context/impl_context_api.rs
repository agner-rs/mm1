use std::collections::HashSet;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::Lease;
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_common::log;
use mm1_common::types::{AnyError, Never};
use mm1_core::context::{
    Bind, BindArgs, BindErrorKind, Fork, ForkErrorKind, InitDone, Linking, Messaging, Now, Quit,
    RecvErrorKind, SendErrorKind, Start, Stop, Watching,
};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_core::tracing::{TraceId, WithTraceIdExt};
use mm1_proto_system::{InitAck, SpawnErrorKind, StartErrorKind, WatchRef};
use mm1_runnable::local::BoxedRunnable;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::{trace, warn};

use crate::config::EffectiveActorConfig;
use crate::registry;
use crate::runtime::container;
use crate::runtime::context::{ActorContext, SubnetContext};
use crate::runtime::sys_call::SysCall;
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg};

impl Quit for ActorContext {
    async fn quit_ok(&mut self) -> Never {
        self.call.invoke(SysCall::Exit(Ok(()))).await;
        std::future::pending().await
    }

    async fn quit_err<E>(&mut self, reason: E) -> Never
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.call.invoke(SysCall::Exit(Err(reason.into()))).await;
        std::future::pending().await
    }
}

impl Fork for ActorContext {
    async fn fork(&mut self) -> Result<Self, ErrorOf<ForkErrorKind>> {
        let Self {
            fork_address: this_address,
            call,
            subnet_context,
            ..
        } = self;

        let fork_lease = {
            let mut subnet_context_locked = subnet_context
                .try_lock()
                .expect("could not lock subnet_context");
            let SubnetContext {
                subnet_pool,
                fork_entries,
                ..
            } = &mut *subnet_context_locked;
            let fork_lease = subnet_pool.lease(NetMask::MAX).map_err(|lease_error| {
                ErrorOf::new(ForkErrorKind::ResourceConstraint, lease_error.to_string())
            })?;
            let should_be_none = fork_entries.insert(fork_lease.address, Default::default());
            assert!(should_be_none.is_none());
            fork_lease
        };
        let fork_address = fork_lease.address;
        trace!("forking {} -> {}", this_address, fork_address);

        call.invoke(SysCall::ForkAdded(fork_address)).await;

        let context = Self {
            subnet_context: subnet_context.clone(),
            fork_address,
            fork_lease: Some(fork_lease),
            ack_to: None,
            call: call.clone(),
        };

        Ok(context)
    }

    async fn run<F, Fut>(self, fun: F)
    where
        F: FnOnce(Self) -> Fut,
        F: Send + 'static,
        Fut: Future + Send + 'static,
    {
        let call = self.call.clone();
        let fut = fun(self)
            .map(|_| ())
            .with_trace_id(TraceId::current())
            .boxed();
        call.invoke(SysCall::Spawn(fut)).await;
    }
}

impl Now for ActorContext {
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }
}

impl Start<BoxedRunnable<Self>> for ActorContext {
    fn spawn(
        &mut self,
        runnable: BoxedRunnable<Self>,
        link: bool,
    ) -> impl Future<Output = Result<Address, ErrorOf<SpawnErrorKind>>> + Send {
        do_spawn(self, runnable, None, link.then_some(self.fork_address))
    }

    fn start(
        &mut self,
        runnable: BoxedRunnable<Self>,
        link: bool,
        start_timeout: Duration,
    ) -> impl Future<Output = Result<Address, ErrorOf<StartErrorKind>>> + Send {
        do_start(self, runnable, link, start_timeout)
    }
}

impl Stop for ActorContext {
    fn exit(&mut self, peer: Address) -> impl Future<Output = bool> + Send {
        let this = self.fork_address;
        let out = do_exit(self, this, peer).is_ok();
        std::future::ready(out)
    }

    fn kill(&mut self, peer: Address) -> impl Future<Output = bool> + Send {
        let out = do_kill(self, peer).is_ok();
        std::future::ready(out)
    }
}

impl Linking for ActorContext {
    fn link(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.fork_address;
        do_link(self, this, peer)
    }

    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.fork_address;
        do_unlink(self, this, peer)
    }

    fn set_trap_exit(&mut self, enable: bool) -> impl Future<Output = ()> + Send {
        do_set_trap_exit(self, enable)
    }
}

impl Watching for ActorContext {
    fn watch(&mut self, peer: Address) -> impl Future<Output = mm1_proto_system::WatchRef> + Send {
        do_watch(self, peer)
    }

    fn unwatch(
        &mut self,
        watch_ref: mm1_proto_system::WatchRef,
    ) -> impl Future<Output = ()> + Send {
        do_unwatch(self, watch_ref)
    }
}

impl InitDone for ActorContext {
    fn init_done(&mut self, address: Address) -> impl Future<Output = ()> + Send {
        do_init_done(self, address)
    }
}

impl Messaging for ActorContext {
    fn address(&self) -> Address {
        self.fork_address
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        let ActorContext {
            fork_address,
            subnet_context,
            ..
        } = self;

        loop {
            let (inbound_envelope_opt, subnet_notify, fork_notify) = {
                let mut subnet_context_locked = subnet_context
                    .try_lock()
                    .expect("could not lock subnet_context");
                let SubnetContext {
                    subnet_notify,
                    rx_priority,
                    rx_regular,
                    fork_entries,
                    bound_subnets,
                    ..
                } = &mut *subnet_context_locked;

                let mut notified_forks = HashSet::new();
                while let Some(message) = rx_priority
                    .try_recv_realtime()
                    .map_err(|e| ErrorOf::new(RecvErrorKind::Closed, e.to_string()))?
                {
                    let message_to = bound_subnets
                        .get(&AddressRange::from(message.to))
                        .copied()
                        .unwrap_or(message.to);
                    let Some(fork_entry) = fork_entries.get_mut(&message_to) else {
                        warn!("no such fork [dst: {}]", message_to);
                        continue
                    };
                    fork_entry.inbox_priority.push_back(message);
                    trace!("subnet received priority message [dst: {}]", message_to);
                    if notified_forks.insert(message_to) {
                        trace!("notifying fork [dst: {}]", message_to);
                        fork_entry.fork_notifiy.notify_one();
                    }
                }
                while let Some(message) = rx_regular
                    .try_recv_realtime()
                    .map_err(|e| ErrorOf::new(RecvErrorKind::Closed, e.to_string()))?
                {
                    let message_to = bound_subnets
                        .get(&AddressRange::from(message.to))
                        .copied()
                        .unwrap_or(message.to);
                    let Some(fork_entry) = fork_entries.get_mut(&message_to) else {
                        warn!("no such fork [dst: {}]", message_to);
                        continue
                    };
                    fork_entry.inbox_regular.push_back(message);
                    trace!("subnet received regular message [dst: {}]", message_to);
                    if notified_forks.insert(message_to) {
                        trace!("notifying fork [dst: {}]", message_to);
                        fork_entry.fork_notifiy.notify_one();
                    }
                }

                let this_fork_entry = fork_entries
                    .get_mut(fork_address)
                    .unwrap_or_else(|| panic!("no fork-entry for {}", fork_address));
                let inbound_envelope_opt = if let Some(priority_message) =
                    this_fork_entry.inbox_priority.pop_front()
                {
                    Some(priority_message.message)
                } else if let Some(regular_message) = this_fork_entry.inbox_regular.pop_front() {
                    Some(regular_message.message)
                } else {
                    None
                };
                (
                    inbound_envelope_opt,
                    subnet_notify.clone(),
                    this_fork_entry.fork_notifiy.clone(),
                )
            };

            if let Some(inbound_envelope) = inbound_envelope_opt {
                break Ok(inbound_envelope)
            } else {
                let subnet_notified = subnet_notify.notified();
                let fork_notified = fork_notify.notified();

                tokio::select! {
                    _ = subnet_notified => (),
                    _ = fork_notified => (),
                }
            }
        }
    }

    async fn close(&mut self) {
        // self.rx_regular.close();
        // self.rx_priority.close();
        todo!()
    }

    fn send(
        &mut self,
        envelope: Envelope,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send {
        std::future::ready(do_send(self, envelope))
    }

    fn forward(
        &mut self,
        to: Address,
        envelope: Envelope,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send {
        std::future::ready(do_forward(self, to, envelope))
    }
}

impl Bind<NetAddress> for ActorContext {
    async fn bind(&mut self, args: BindArgs<NetAddress>) -> Result<(), ErrorOf<BindErrorKind>> {
        use std::collections::btree_map::Entry::*;

        let BindArgs {
            bind_to,
            inbox_size,
        } = args;
        let address_range = AddressRange::from(bind_to);

        log::debug!("binding [to: {}; inbox-size: {}]", bind_to, inbox_size);

        let Self {
            fork_address,
            subnet_context,
            ..
        } = self;

        {
            let mut subnet_context_locked = subnet_context
                .try_lock()
                .expect("could not lock subnet_context");

            let SubnetContext {
                rt_api,
                subnet_mailbox_tx,
                bound_subnets,
                ..
            } = &mut *subnet_context_locked;

            let bound_subnet_entry = match bound_subnets.entry(address_range) {
                Vacant(v) => v,
                Occupied(o) => {
                    let previously_bound_to = NetAddress::from(*o.key());
                    return Err(ErrorOf::new(
                        BindErrorKind::Conflict,
                        format!("conflict [requested: {bind_to}; existing: {previously_bound_to}]"),
                    ))
                },
            };
            let subnet_lease = Lease::trusted(bind_to);

            let subnet_mailbox_tx = subnet_mailbox_tx.upgrade().ok_or(ErrorOf::new(
                BindErrorKind::Closed,
                "the actor subnet is probably unregistered",
            ))?;
            let bound_subnet_node =
                registry::Node::new(subnet_lease, inbox_size, subnet_mailbox_tx);

            rt_api
                .registry()
                .register(bind_to, bound_subnet_node)
                .map_err(|_| {
                    ErrorOf::new(
                        BindErrorKind::Conflict,
                        "could not register the subnet-node",
                    )
                })?;

            bound_subnet_entry.insert(*fork_address);
        }

        log::info!("bound [to: {}; inbox-size: {}]", bind_to, inbox_size);

        Ok(())
    }
}

async fn do_start(
    context: &mut ActorContext,
    runnable: BoxedRunnable<ActorContext>,
    link: bool,
    timeout: Duration,
) -> Result<Address, ErrorOf<StartErrorKind>> {
    let this_address = context.fork_address;

    let mut fork = context
        .fork()
        .await
        .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?;

    let fork_address = fork.fork_address;
    let spawned_address = do_spawn(&mut fork, runnable, Some(fork_address), Some(fork_address))
        .await
        .map_err(|e| e.map_kind(StartErrorKind::Spawn))?;

    let envelope = match fork.recv().timeout(timeout).await {
        Err(_elapsed) => {
            do_kill(context, spawned_address)
                .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?;

            // TODO: should we ensure termination with a `system::Watch`?

            return Err(ErrorOf::new(
                StartErrorKind::Timeout,
                "no init-ack within timeout",
            ))
        },
        Ok(recv_result) => {
            recv_result.map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?
        },
    };

    dispatch!(match envelope {
        mm1_proto_system::InitAck { address } => {
            if link {
                do_link(context, this_address, address).await;
            }
            Ok(address)
        },

        mm1_proto_system::Exited { .. } => {
            Err(ErrorOf::new(
                StartErrorKind::Exited,
                "exited before init-ack",
            ))
        },

        unexpected @ _ => {
            Err(ErrorOf::new(
                StartErrorKind::InternalError,
                format!("unexpected message: {unexpected:?}"),
            ))
        },
    })
}

async fn do_spawn(
    context: &mut ActorContext,
    runnable: BoxedRunnable<ActorContext>,
    ack_to: Option<Address>,
    link_to: impl IntoIterator<Item = Address>,
) -> Result<Address, ErrorOf<SpawnErrorKind>> {
    let ActorContext { subnet_context, .. } = context;

    let (actor_key, rt_config, rt_api, tx_actor_failure) = {
        let subnet_context_locked = subnet_context
            .try_lock()
            .expect("could not lock subnet_context");
        let SubnetContext {
            rt_api,
            rt_config,
            actor_key,
            tx_actor_failure,
            ..
        } = &*subnet_context_locked;

        (
            actor_key.child(runnable.func_name()),
            rt_config.clone(),
            rt_api.clone(),
            tx_actor_failure.clone(),
        )
    };

    let actor_config = rt_config.actor_config(&actor_key);
    let execute_on = rt_api.choose_executor(actor_config.runtime_key());

    trace!("starting [ack-to: {:?}]", ack_to);

    let subnet_lease = rt_api
        .request_address(actor_config.netmask())
        .await
        .inspect_err(|e| log::error!("lease-error: {}", e))
        .map_err(|e| ErrorOf::new(SpawnErrorKind::ResourceConstraint, e.to_string()))?;

    trace!("starting [subnet-lease: {}]", subnet_lease.net_address());

    let rt_api = rt_api.clone();
    let rt_config = rt_config.clone();
    let container = container::Container::create(
        container::ContainerArgs {
            ack_to,
            // FIXME: can we make it IntoIterator too?
            link_to: link_to.into_iter().collect(),
            actor_key,
            trace_id: TraceId::current(),

            subnet_lease,
            rt_api,
            rt_config,
        },
        runnable,
        tx_actor_failure.clone(),
    )
    .map_err(|e| ErrorOf::new(SpawnErrorKind::InternalError, e.to_string()))?;
    let actor_address = container.actor_address();

    trace!("actor-address: {}", actor_address);

    let tx_actor_failure = tx_actor_failure.clone();
    // TODO: maybe keep it somewhere too?
    let _join_handle = execute_on.spawn(async move {
        match container.run().await {
            Ok(Ok(())) => (),
            Ok(Err(actor_failure)) => {
                let _ = tx_actor_failure.send((actor_address, actor_failure));
            },
            Err(container_failure) => {
                let report = AnyError::from(container_failure);
                mm1_common::log::error!(
                    "actor container failure [addr: {}]: {}",
                    actor_address,
                    report
                        .chain()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join(" <- ")
                );
            },
        }
    });

    Ok(actor_address)
}

fn do_exit(context: &mut ActorContext, this: Address, peer: Address) -> Result<(), SendErrorKind> {
    let ActorContext { subnet_context, .. } = context;
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;

    rt_api.sys_send(
        peer,
        SysMsg::Link(SysLink::Exit {
            sender:   this,
            receiver: peer,
            reason:   ExitReason::Terminate,
        }),
    )
}

fn do_kill(context: &mut ActorContext, peer: Address) -> Result<(), SendErrorKind> {
    let ActorContext { subnet_context, .. } = context;
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;

    rt_api.sys_send(peer, SysMsg::Kill)
}

async fn do_link(context: &mut ActorContext, this: Address, peer: Address) {
    let ActorContext { call, .. } = context;

    call.invoke(SysCall::Link {
        sender:   this,
        receiver: peer,
    })
    .await
}

async fn do_unlink(context: &mut ActorContext, this: Address, peer: Address) {
    let ActorContext { call, .. } = context;
    call.invoke(SysCall::Unlink {
        sender:   this,
        receiver: peer,
    })
    .await
}

async fn do_set_trap_exit(context: &mut ActorContext, enable: bool) {
    let ActorContext { call, .. } = context;
    call.invoke(SysCall::TrapExit(enable)).await;
}

async fn do_watch(context: &mut ActorContext, peer: Address) -> WatchRef {
    let ActorContext {
        fork_address: this,
        call,
        ..
    } = context;
    let (reply_tx, reply_rx) = oneshot::channel();
    call.invoke(SysCall::Watch {
        sender: *this,
        receiver: peer,
        reply_tx,
    })
    .await;
    reply_rx.await.expect("sys-call remained unanswered")
}

async fn do_unwatch(context: &mut ActorContext, watch_ref: WatchRef) {
    let ActorContext {
        fork_address: this,
        call,
        ..
    } = context;
    call.invoke(SysCall::Unwatch {
        sender: *this,
        watch_ref,
    })
    .await;
}

async fn do_init_done(context: &mut ActorContext, address: Address) {
    let ActorContext {
        ack_to,
        subnet_context,
        ..
    } = context;
    let message = InitAck { address };
    let Some(ack_to_address) = ack_to.take() else {
        return;
    };
    let envelope = Envelope::new(EnvelopeHeader::to_address(ack_to_address), message);
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;
    let _ = rt_api.send_to(envelope.header().to, true, envelope.into_erased());
}

fn do_send(context: &mut ActorContext, outbound: Envelope) -> Result<(), ErrorOf<SendErrorKind>> {
    trace!("sending [outbound: {:?}]", outbound);
    let ActorContext { subnet_context, .. } = context;
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;
    rt_api
        .send_to(outbound.header().to, outbound.header().priority, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}

fn do_forward(
    context: &mut ActorContext,
    to: Address,
    outbound: Envelope,
) -> Result<(), ErrorOf<SendErrorKind>> {
    trace!("forwarding [to: {}, outbound: {:?}]", to, outbound);
    let ActorContext { subnet_context, .. } = context;
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;
    rt_api
        .send_to(to, outbound.header().priority, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}
