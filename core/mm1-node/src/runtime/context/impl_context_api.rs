use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::Lease;
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_common::errors::chain::{ExactTypeDisplayChainExt, StdErrorDisplayChainExt};
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_common::log;
use mm1_common::types::{AnyError, Never};
use mm1_core::context::{
    Bind, BindArgs, BindErrorKind, Fork, ForkErrorKind, InitDone, Linking, Messaging, Now, Ping,
    PingErrorKind, Quit, RecvErrorKind, SendErrorKind, Start, Stop, Watching,
};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_core::tap;
use mm1_core::tracing::{TraceId, WithTraceIdExt};
use mm1_proto_system as sys;
use mm1_runnable::local::BoxedRunnable;
use rand::RngCore;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::{trace, warn};

use crate::config::EffectiveActorConfig;
use crate::registry::{self, MessageWithPermit, MessageWithoutPermit};
use crate::runtime::container;
use crate::runtime::context::{ActorContext, ForkEntry, SubnetContext};
use crate::runtime::rt_api::RtApi;
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
        trace!(parent = %this_address, child = %fork_address, "forking");

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
    ) -> impl Future<Output = Result<Address, ErrorOf<sys::SpawnErrorKind>>> + Send {
        do_spawn(self, runnable, None, link.then_some(self.fork_address))
    }

    fn start(
        &mut self,
        runnable: BoxedRunnable<Self>,
        link: bool,
        start_timeout: Duration,
    ) -> impl Future<Output = Result<Address, ErrorOf<sys::StartErrorKind>>> + Send {
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
                    rt_api,
                    actor_key,
                    subnet_address,
                    subnet_notify,
                    rx_priority,
                    rx_regular,
                    fork_entries,
                    bound_subnets,
                    message_tap,
                    ..
                } = &mut *subnet_context_locked;

                process_inlets(rt_api, bound_subnets, rx_priority, rx_regular, fork_entries)?;

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
                if let Some(envelope) = inbound_envelope_opt.as_ref() {
                    message_tap.on_recv(tap::OnRecv {
                        recv_by_addr: *fork_address,
                        recv_by_net: *subnet_address,
                        recv_by_key: actor_key,
                        envelope,
                    });
                }
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

        log::debug!(%bind_to, %inbox_size, "binding");

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

        log::info!(%bind_to, %inbox_size, "bound");

        Ok(())
    }
}

impl Ping for ActorContext {
    async fn ping(
        &mut self,
        address: Address,
        timeout: Duration,
    ) -> Result<Duration, ErrorOf<PingErrorKind>> {
        let ping_id = rand::rng().next_u64();
        let now = Instant::now();
        let deadline = now.checked_add(timeout).unwrap_or(now);

        let Self {
            fork_address,
            subnet_context,
            ..
        } = self;

        let (subnet_notify, fork_notify) = {
            let mut subnet_context_locked = subnet_context
                .try_lock()
                .expect("could not lock subnet_context");
            let SubnetContext {
                rt_api,
                fork_entries,
                subnet_notify,
                ..
            } = &mut *subnet_context_locked;
            let ForkEntry { fork_notifiy, .. } = fork_entries
                .get_mut(fork_address)
                .expect("fork_entry missing");

            let ping_msg = sys::Ping {
                reply_to: Some(*fork_address),
                id:       ping_id,
            };

            let ping_header = EnvelopeHeader::to_address(address).with_priority(true);
            let ping_envelope = Envelope::new(ping_header, ping_msg).into_erased();
            rt_api
                .send_to(address, true, ping_envelope)
                .map_err(|e| ErrorOf::new(PingErrorKind::Send, e.to_string()))?;

            (subnet_notify.clone(), fork_notifiy.clone())
        };

        loop {
            let timeout = tokio::time::sleep_until(deadline);
            tokio::select! {
                _ = timeout => { return Err(ErrorOf::new(PingErrorKind::Timeout, "timeout elapsed")) },
                _ = subnet_notify.notified() => (),
                _ = fork_notify.notified() => (),
            };

            let mut subnet_context_locked = subnet_context
                .try_lock()
                .expect("could not lock subnet_context");
            let SubnetContext {
                rt_api,
                fork_entries,
                bound_subnets,
                rx_priority,
                rx_regular,
                ..
            } = &mut *subnet_context_locked;

            process_inlets(rt_api, bound_subnets, rx_priority, rx_regular, fork_entries)
                .map_err(|e| ErrorOf::new(PingErrorKind::Recv, e.to_string()))?;

            if fork_entries
                .get(fork_address)
                .expect("fork_entry_missing")
                .last_ping_received
                == Some(ping_id)
            {
                break
            }
        }
        Ok(now.elapsed())
    }
}

fn process_inlets(
    rt_api: &RtApi,
    bound_subnets: &BTreeMap<AddressRange, Address>,
    rx_priority: &mut kanal::Receiver<MessageWithoutPermit<Envelope>>,
    rx_regular: &mut kanal::Receiver<MessageWithPermit<Envelope>>,
    fork_entries: &mut HashMap<Address, ForkEntry>,
) -> Result<(), ErrorOf<RecvErrorKind>> {
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
            warn!(dst = %message_to, "no such fork");
            continue
        };
        let should_notify = if let Some(ping_message) = message.message.peek::<sys::Ping>() {
            let sys::Ping { reply_to, id } = *ping_message;
            if let Some(reply_to) = reply_to {
                trace!(%id, %reply_to, "received a ping request");
                let pong_header = EnvelopeHeader::to_address(reply_to).with_priority(true);
                let pong_envelope =
                    Envelope::new(pong_header, sys::Ping { reply_to: None, id }).into_erased();
                rt_api
                    .send_to(reply_to, true, pong_envelope)
                    .inspect_err(
                        |e| warn!(reason = %e.as_display_chain(), "can't send ping-response"),
                    )
                    .ok();
                trace!(%id, %reply_to, "send a ping response");
                false
            } else {
                trace!(%id, "received a ping response");
                fork_entry.last_ping_received = Some(id);
                true
            }
        } else {
            fork_entry.inbox_priority.push_back(message);
            true
        };

        if should_notify && notified_forks.insert(message_to) {
            trace!(dst = %message_to, "notifying fork");
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
            warn!(dst = %message_to, "no such fork");
            continue
        };
        fork_entry.inbox_regular.push_back(message);
        trace!(dst = %message_to, "subnet received regular message");
        if notified_forks.insert(message_to) {
            trace!(dst = %message_to, "notifying fork");
            fork_entry.fork_notifiy.notify_one();
        }
    }

    Ok(())
}

async fn do_start(
    context: &mut ActorContext,
    runnable: BoxedRunnable<ActorContext>,
    link: bool,
    timeout: Duration,
) -> Result<Address, ErrorOf<sys::StartErrorKind>> {
    let this_address = context.fork_address;

    let mut fork = context
        .fork()
        .await
        .map_err(|e| ErrorOf::new(sys::StartErrorKind::InternalError, e.to_string()))?;

    let fork_address = fork.fork_address;
    let spawned_address = do_spawn(&mut fork, runnable, Some(fork_address), Some(fork_address))
        .await
        .map_err(|e| e.map_kind(sys::StartErrorKind::Spawn))?;

    let envelope = match fork.recv().timeout(timeout).await {
        Err(_elapsed) => {
            do_kill(context, spawned_address)
                .map_err(|e| ErrorOf::new(sys::StartErrorKind::InternalError, e.to_string()))?;

            // TODO: should we ensure termination with a `system::Watch`?

            return Err(ErrorOf::new(
                sys::StartErrorKind::Timeout,
                "no init-ack within timeout",
            ))
        },
        Ok(recv_result) => {
            recv_result
                .map_err(|e| ErrorOf::new(sys::StartErrorKind::InternalError, e.to_string()))?
        },
    };

    dispatch!(match envelope {
        sys::InitAck { address } => {
            if link {
                do_link(context, this_address, address).await;
            }
            Ok(address)
        },

        sys::Exited { .. } => {
            Err(ErrorOf::new(
                sys::StartErrorKind::Exited,
                "exited before init-ack",
            ))
        },

        unexpected @ _ => {
            Err(ErrorOf::new(
                sys::StartErrorKind::InternalError,
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
) -> Result<Address, ErrorOf<sys::SpawnErrorKind>> {
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
    let message_tap = rt_api.message_tap(actor_config.message_tap_key());

    trace!(?ack_to, "starting");

    let subnet_lease = rt_api
        .request_address(actor_config.netmask())
        .await
        .inspect_err(|e| log::error!("lease-error: {}", e))
        .map_err(|e| ErrorOf::new(sys::SpawnErrorKind::ResourceConstraint, e.to_string()))?;

    trace!(subnet_lease = %subnet_lease.net_address(), "subnet leased");

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
            message_tap,
            tx_actor_failure: tx_actor_failure.clone(),
        },
        runnable,
    )
    .map_err(|e| ErrorOf::new(sys::SpawnErrorKind::InternalError, e.to_string()))?;
    let actor_address = container.actor_address();

    trace!(spawned_address = %actor_address, "about to run spawned actor");

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
                log::error!(
                    err = %report.as_display_chain(), %actor_address,
                    "actor container failure"
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

async fn do_watch(context: &mut ActorContext, peer: Address) -> sys::WatchRef {
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

async fn do_unwatch(context: &mut ActorContext, watch_ref: sys::WatchRef) {
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
    let message = sys::InitAck { address };
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
    let sender = context.fork_address;
    let to = outbound.header().to;
    {
        let subnet_context_locked = context
            .subnet_context
            .try_lock()
            .expect("could not lock subnet_context");
        let SubnetContext {
            subnet_address,
            actor_key,
            message_tap,
            ..
        } = &*subnet_context_locked;
        message_tap.on_send(tap::OnSend {
            sent_by_addr: sender,
            sent_by_net:  *subnet_address,
            sent_by_key:  actor_key,
            sent_to_addr: to,
            envelope:     &outbound,
        });
    }

    let (message, empty_envelope) = outbound.take();
    let mut header: EnvelopeHeader = empty_envelope.into();

    let new_ttl = header.ttl.checked_sub(1).ok_or_else(|| {
        warn!(
            envelope_header = ?header,
            "TTL exhausted, dropping message"
        );
        ErrorOf::new(SendErrorKind::TtlExhausted, "TTL exhausted")
    })?;
    header.ttl = new_ttl;

    let outbound = Envelope::new(header, message);

    trace!(envelope = ?outbound, "sending");
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
    let sender = context.fork_address;
    {
        let subnet_context_locked = context
            .subnet_context
            .try_lock()
            .expect("could not lock subnet_context");
        let SubnetContext {
            subnet_address,
            actor_key,
            message_tap,
            ..
        } = &*subnet_context_locked;
        message_tap.on_send(tap::OnSend {
            sent_by_addr: sender,
            sent_by_net:  *subnet_address,
            sent_by_key:  actor_key,
            sent_to_addr: to,
            envelope:     &outbound,
        });
    }

    let (message, empty_envelope) = outbound.take();
    let mut header: EnvelopeHeader = empty_envelope.into();

    let new_ttl = header.ttl.checked_sub(1).ok_or_else(|| {
        warn!(
            forward_to = %to,
            envelope_header = ?header,
            "TTL exhausted, dropping message"
        );
        ErrorOf::new(SendErrorKind::TtlExhausted, "TTL exhausted")
    })?;
    header.ttl = new_ttl;

    let outbound = Envelope::new(header, message);

    trace!(forward_to = %to, envelope = ?outbound, "forwarding");
    let ActorContext { subnet_context, .. } = context;
    let subnet_context_locked = subnet_context
        .try_lock()
        .expect("could not lock subnet_context");
    let SubnetContext { rt_api, .. } = &*subnet_context_locked;
    rt_api
        .send_to(to, outbound.header().priority, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}
