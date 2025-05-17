use mm1_common::errors::error_of::ErrorOf;
use mm1_core::context::Call;
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_proto_system::{
    Exit, InitAck, Kill, Link, SpawnErrorKind, SpawnRequest, SpawnResponse, TrapExit, Unlink,
    Unwatch, Watch, WatchRef,
};
use tokio::sync::oneshot;
use tracing::trace;

use crate::runtime::config::EffectiveActorConfig;
use crate::runtime::container;
use crate::runtime::context::ActorContext;
use crate::runtime::sys_call::SysCall;
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg};
use crate::runtime::system::Local;

impl Call<Local, SpawnRequest<Local>> for ActorContext {
    type Outcome = SpawnResponse;

    async fn call(&mut self, _to: Local, message: SpawnRequest<Local>) -> Self::Outcome {
        let SpawnRequest {
            runnable,
            ack_to,
            link_to,
        } = message;

        let actor_key = self.actor_key.child(runnable.func_name());
        let actor_config = self.rt_config.actor_config(&actor_key);
        let execute_on = self.rt_api.choose_executor(actor_config.runtime_key());

        trace!("starting [ack-to: {:?}; link-to: {:?}]", ack_to, link_to);

        let subnet_lease = self
            .rt_api
            .request_address(actor_config.netmask())
            .await
            .map_err(|e| ErrorOf::new(SpawnErrorKind::ResourceConstraint, e.to_string()))?;

        trace!("subnet-lease: {}", subnet_lease.net_address());

        let rt_api = self.rt_api.clone();
        let rt_config = self.rt_config.clone();
        let container = container::Container::create(
            container::ContainerArgs {
                ack_to,
                link_to,
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
}

impl Call<Local, Kill> for ActorContext {
    type Outcome = bool;

    async fn call(&mut self, _to: Local, message: Kill) -> Self::Outcome {
        let Kill { peer: address } = message;

        self.rt_api.sys_send(address, SysMsg::Kill).is_ok()
    }
}

impl Call<Local, InitAck> for ActorContext {
    type Outcome = ();

    async fn call(&mut self, _to: Local, message: InitAck) -> Self::Outcome {
        let Some(ack_to_address) = self.ack_to.take() else {
            return;
        };
        let envelope = Envelope::new(EnvelopeInfo::new(ack_to_address), message);
        let _ = self.rt_api.send(true, envelope.into_erased());
    }
}

impl Call<Local, TrapExit> for ActorContext {
    type Outcome = ();

    async fn call(&mut self, _to: Local, message: TrapExit) -> Self::Outcome {
        let TrapExit { enable } = message;

        self.call.invoke(SysCall::TrapExit(enable)).await;
    }
}

impl Call<Local, Link> for ActorContext {
    type Outcome = ();

    async fn call(&mut self, _to: Local, message: Link) -> Self::Outcome {
        let Link { peer } = message;

        self.call
            .invoke(SysCall::Link {
                sender:   self.actor_address,
                receiver: peer,
            })
            .await;
    }
}

impl Call<Local, Unlink> for ActorContext {
    type Outcome = ();

    async fn call(&mut self, _to: Local, message: Unlink) -> Self::Outcome {
        let Unlink { peer } = message;

        self.call
            .invoke(SysCall::Unlink {
                sender:   self.actor_address,
                receiver: peer,
            })
            .await;
    }
}

impl Call<Local, Exit> for ActorContext {
    type Outcome = bool;

    async fn call(&mut self, _to: Local, msg: Exit) -> Self::Outcome {
        let Exit { peer } = msg;
        self.rt_api
            .sys_send(
                peer,
                SysMsg::Link(SysLink::Exit {
                    sender:   self.actor_address,
                    receiver: peer,
                    reason:   ExitReason::Terminate,
                }),
            )
            .is_ok()
    }
}

impl Call<Local, Watch> for ActorContext {
    type Outcome = WatchRef;

    async fn call(&mut self, _to: Local, msg: Watch) -> Self::Outcome {
        let Watch { peer } = msg;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.call
            .invoke(SysCall::Watch {
                sender: self.actor_address,
                receiver: peer,
                reply_tx,
            })
            .await;
        reply_rx.await.expect("sys-call remained unanswered")
    }
}

impl Call<Local, Unwatch> for ActorContext {
    type Outcome = ();

    async fn call(&mut self, _to: Local, msg: Unwatch) -> Self::Outcome {
        let Unwatch { watch_ref } = msg;
        self.call
            .invoke(SysCall::Unwatch {
                sender: self.actor_address,
                watch_ref,
            })
            .await;
    }
}
