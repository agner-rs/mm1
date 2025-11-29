use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::Lease;
use mm1_address::subnet::NetAddress;
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
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::trace;

use crate::config::EffectiveActorConfig;
use crate::registry::{ForkEntry, NetworkNode};
use crate::runtime::container;
use crate::runtime::context::ActorContext;
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
        let actor_node = self
            .actor_node
            .upgrade()
            .ok_or_else(|| ErrorOf::new(ForkErrorKind::InternalError, "actor_node.upgrade"))?;
        let fork_lease = actor_node.lease_address().map_err(|lease_error| {
            ErrorOf::new(ForkErrorKind::ResourceConstraint, lease_error.to_string())
        })?;
        let fork_address = fork_lease.address;

        let (tx_priority, rx_priority) = mpsc::unbounded_channel();
        let (tx_regular, rx_regular) = mpsc::unbounded_channel();
        let tx_system_weak = self.tx_system_weak.clone();

        self.call
            .invoke(SysCall::ForkAdded(
                fork_lease.address,
                tx_priority.downgrade(),
            ))
            .await;

        let tx_priority_weak = tx_priority.downgrade();
        let tx_regular_weak = tx_regular.downgrade();
        let fork_entry = ForkEntry::new(fork_lease, tx_priority, tx_regular);

        let registered = actor_node.register(fork_address, fork_entry);
        assert!(registered);

        let rt_api = self.rt_api.clone();
        let rt_config = self.rt_config.clone();

        let actor_key = self.actor_key.clone();

        let context = Self {
            rt_api,
            rt_config,

            address: fork_address,
            actor_key,
            ack_to: None,
            actor_node: self.actor_node.clone(),
            network_nodes: Default::default(),

            rx_priority,
            rx_regular,
            tx_system_weak,
            tx_priority_weak,
            tx_regular_weak,
            call: self.call.clone(),

            tx_actor_failure: self.tx_actor_failure.clone(),
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
        do_spawn(self, runnable, None, link.then_some(self.address))
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
        let this = self.address;
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
        let this = self.address;
        do_link(self, this, peer)
    }

    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.address;
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
        self.address
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        let (priority, inbound_opt) = tokio::select! {
            biased;

            inbound_opt = self.rx_priority.recv() => (true, inbound_opt),
            inbound_opt = self.rx_regular.recv() => (false, inbound_opt.map(|m| m.message)),
        };

        trace!(
            "received [priority: {}; inbound: {:?}; via {}]",
            priority, inbound_opt, self.address
        );

        inbound_opt.ok_or(ErrorOf::new(RecvErrorKind::Closed, "closed"))
    }

    async fn close(&mut self) {
        self.rx_regular.close();
        self.rx_priority.close();
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

        log::debug!("binding [to: {}; inbox-size: {}]", bind_to, inbox_size);

        let rt_api = self.rt_api.clone();
        let registry = rt_api.registry();

        let address_range = AddressRange::from(bind_to);

        let network_nodes_entry = match self.network_nodes.entry(address_range) {
            Occupied(o) => {
                let previously_bound_to = NetAddress::from(*o.key());
                return Err(ErrorOf::new(
                    BindErrorKind::Conflict,
                    format!("conflict [requested: {bind_to}; existing: {previously_bound_to}]"),
                ))
            },
            Vacant(v) => v,
        };

        let subnet_lease = Lease::trusted(bind_to);

        let (tx_system, _rx_system) = mpsc::unbounded_channel();

        let tx_priority = self.tx_priority_weak.upgrade().ok_or_else(|| {
            ErrorOf::new(BindErrorKind::Closed, "tx_priority_weak.upgrade failed")
        })?;
        let tx_regular = self.tx_regular_weak.upgrade().ok_or_else(|| {
            ErrorOf::new(BindErrorKind::Closed, "tx_priority_weak.upgrade failed")
        })?;

        let network_node = Arc::new(NetworkNode::new(
            subnet_lease,
            inbox_size,
            tx_system,
            tx_priority,
            tx_regular,
        ));

        if !registry.register(bind_to, network_node.clone()) {
            log::warn!("couldn't bind [to: {}; reason: conflict]", bind_to);
            return Err(ErrorOf::new(
                BindErrorKind::Conflict,
                "probably address in use",
            ))
        }

        let network_node = Arc::downgrade(&network_node);
        network_nodes_entry.insert(network_node);

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
    let this_address = context.address;

    let mut fork = context
        .fork()
        .await
        .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?;

    let fork_address = fork.address;
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
    let actor_key = context.actor_key.child(runnable.func_name());
    let actor_config = context.rt_config.actor_config(&actor_key);
    let execute_on = context.rt_api.choose_executor(actor_config.runtime_key());

    trace!("starting [ack-to: {:?}]", ack_to);

    let subnet_lease = context
        .rt_api
        .request_address(actor_config.netmask())
        .await
        .map_err(|e| ErrorOf::new(SpawnErrorKind::ResourceConstraint, e.to_string()))?;

    let rt_api = context.rt_api.clone();
    let rt_config = context.rt_config.clone();
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
        context.tx_actor_failure.clone(),
    )
    .map_err(|e| ErrorOf::new(SpawnErrorKind::InternalError, e.to_string()))?;
    let actor_address = container.actor_address();

    trace!("actor-address: {}", actor_address);

    let tx_actor_failure = context.tx_actor_failure.clone();
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
    context.rt_api.sys_send(
        peer,
        SysMsg::Link(SysLink::Exit {
            sender:   this,
            receiver: peer,
            reason:   ExitReason::Terminate,
        }),
    )
}

fn do_kill(context: &mut ActorContext, peer: Address) -> Result<(), SendErrorKind> {
    context.rt_api.sys_send(peer, SysMsg::Kill)
}

async fn do_link(context: &mut ActorContext, this: Address, peer: Address) {
    context
        .call
        .invoke(SysCall::Link {
            sender:   this,
            receiver: peer,
        })
        .await
}

async fn do_unlink(context: &mut ActorContext, this: Address, peer: Address) {
    context
        .call
        .invoke(SysCall::Unlink {
            sender:   this,
            receiver: peer,
        })
        .await
}

async fn do_set_trap_exit(context: &mut ActorContext, enable: bool) {
    context.call.invoke(SysCall::TrapExit(enable)).await;
}

async fn do_watch(context: &mut ActorContext, peer: Address) -> WatchRef {
    let this = context.address;
    let (reply_tx, reply_rx) = oneshot::channel();
    context
        .call
        .invoke(SysCall::Watch {
            sender: this,
            receiver: peer,
            reply_tx,
        })
        .await;
    reply_rx.await.expect("sys-call remained unanswered")
}

async fn do_unwatch(context: &mut ActorContext, watch_ref: WatchRef) {
    let this = context.address;
    context
        .call
        .invoke(SysCall::Unwatch {
            sender: this,
            watch_ref,
        })
        .await;
}

async fn do_init_done(context: &mut ActorContext, address: Address) {
    let message = InitAck { address };
    let Some(ack_to_address) = context.ack_to.take() else {
        return;
    };
    let envelope = Envelope::new(EnvelopeHeader::to_address(ack_to_address), message);
    let _ = context
        .rt_api
        .send_to(envelope.header().to, true, envelope.into_erased());
}

fn do_send(context: &mut ActorContext, outbound: Envelope) -> Result<(), ErrorOf<SendErrorKind>> {
    trace!("sending [outbound: {:?}]", outbound);
    context
        .rt_api
        .send_to(outbound.header().to, false, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}

fn do_forward(
    context: &mut ActorContext,
    to: Address,
    outbound: Envelope,
) -> Result<(), ErrorOf<SendErrorKind>> {
    trace!("forwarding [to: {}, outbound: {:?}]", to, outbound);
    context
        .rt_api
        .send_to(to, false, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}
