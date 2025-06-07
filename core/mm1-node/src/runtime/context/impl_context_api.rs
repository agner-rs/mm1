use std::future::Future;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::subnet::NetMask;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_common::types::Never;
use mm1_core::context::{
    Fork, ForkErrorKind, InitDone, Linking, Messaging, Now, Quit, RecvErrorKind, SendErrorKind,
    Start, Stop, Watching,
};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_proto_system::{InitAck, SpawnErrorKind, StartErrorKind, WatchRef};
use mm1_runnable::local::BoxedRunnable;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tracing::trace;

use crate::runtime::config::EffectiveActorConfig;
use crate::runtime::context::ActorContext;
use crate::runtime::sys_call::SysCall;
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg};
use crate::runtime::{container, mq};

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
        let address_lease = self
            .subnet_pool
            .lease(NetMask::M_64)
            .map_err(|lease_error| {
                ErrorOf::new(ForkErrorKind::ResourceConstraint, lease_error.to_string())
            })?;
        let actor_address = address_lease.address;

        let (tx_priority, rx_priority) = mq::unbounded();
        let (tx_regular, rx_regular) = mq::bounded(
            self.rt_config
                .actor_config(&self.actor_key)
                .fork_inbox_size(),
        );
        let tx_system_weak = self.tx_system_weak.clone();
        let tx_system = tx_system_weak
            .upgrade()
            .ok_or_else(|| ErrorOf::new(ForkErrorKind::InternalError, "tx_system_weak.upgrade"))?;

        self.call
            .invoke(SysCall::ForkAdded(
                address_lease.address,
                tx_priority.downgrade(),
            ))
            .await;

        let () = self
            .rt_api
            .register(address_lease, tx_system, tx_priority, tx_regular);

        let subnet_pool = self.subnet_pool.clone();
        let rt_api = self.rt_api.clone();
        let rt_config = self.rt_config.clone();

        let actor_key = self.actor_key.clone();

        let context = Self {
            rt_api,
            rt_config,
            actor_address,
            rx_priority,
            rx_regular,
            tx_system_weak,
            call: self.call.clone(),
            actor_key,
            subnet_pool,
            ack_to: None,
            unregister_on_drop: true,
        };

        Ok(context)
    }

    async fn run<F, Fut>(self, fun: F)
    where
        F: FnOnce(Self) -> Fut,
        F: Send + 'static,
        Fut: std::future::Future + Send + 'static,
    {
        let call = self.call.clone();
        let fut = fun(self).map(|_| ()).boxed();
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
        do_spawn(self, runnable, None, link.then_some(self.actor_address))
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
        let this = self.actor_address;
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
        let this = self.actor_address;
        do_link(self, this, peer)
    }

    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.actor_address;
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
        self.actor_address
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        let (priority, inbound_opt) = tokio::select! {
            biased;

            inbound_opt = self.rx_priority.recv() => (true, inbound_opt),
            inbound_opt = self.rx_regular.recv() => (false, inbound_opt),
        };

        trace!(
            "received [priority: {}; inbound: {:?}]",
            priority, inbound_opt
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
}

async fn do_start(
    context: &mut ActorContext,
    runnable: BoxedRunnable<ActorContext>,
    link: bool,
    timeout: Duration,
) -> Result<Address, ErrorOf<StartErrorKind>> {
    let this_address = context.actor_address;

    let mut fork = context
        .fork()
        .await
        .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?;

    let fork_address = fork.actor_address;
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
                format!("unexpected message: {:?}", unexpected),
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

    trace!("subnet-lease: {}", subnet_lease.net_address());

    let rt_api = context.rt_api.clone();
    let rt_config = context.rt_config.clone();
    let container = container::Container::create(
        container::ContainerArgs {
            ack_to,
            // FIXME: can we make it IntoIterator too?
            link_to: link_to.into_iter().collect(),
            actor_key,

            subnet_lease,
            rt_api,
            rt_config,
        },
        runnable,
    )
    .map_err(|e| ErrorOf::new(SpawnErrorKind::InternalError, e.to_string()))?;
    let actor_address = container.actor_address();

    trace!("actor-address: {}", actor_address);

    // TODO: maybe keep it somewhere too?
    let _join_handle = execute_on.spawn(container.run());

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
    let this = context.actor_address;
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
    let this = context.actor_address;
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
    let _ = context.rt_api.send(true, envelope.into_erased());
}

fn do_send(context: &mut ActorContext, outbound: Envelope) -> Result<(), ErrorOf<SendErrorKind>> {
    trace!("sending [outbound: {:?}]", outbound);
    context
        .rt_api
        .send(false, outbound)
        .map_err(|k| ErrorOf::new(k, ""))
}
