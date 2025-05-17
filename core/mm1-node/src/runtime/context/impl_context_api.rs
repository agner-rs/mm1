use std::future::Future;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_core::context::{Fork, InitDone, Linking, Recv, Start, Stop, TellErrorKind, Watching};
use mm1_core::envelope::{Envelope, EnvelopeInfo, dispatch};
use mm1_proto_system::{InitAck, SpawnErrorKind, StartErrorKind, WatchRef};
use tokio::sync::oneshot;
use tracing::trace;

use crate::runtime::config::EffectiveActorConfig;
use crate::runtime::container;
use crate::runtime::context::ActorContext;
use crate::runtime::runnable::BoxedRunnable;
use crate::runtime::sys_call::SysCall;
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg};

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

fn do_exit(context: &mut ActorContext, this: Address, peer: Address) -> Result<(), TellErrorKind> {
    context.rt_api.sys_send(
        peer,
        SysMsg::Link(SysLink::Exit {
            sender:   this,
            receiver: peer,
            reason:   ExitReason::Terminate,
        }),
    )
}

fn do_kill(context: &mut ActorContext, peer: Address) -> Result<(), TellErrorKind> {
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
    let envelope = Envelope::new(EnvelopeInfo::new(ack_to_address), message);
    let _ = context.rt_api.send(true, envelope.into_erased());
}
