use std::sync::Arc;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::pool::Pool as SubnetPool;
use mm1_address::subnet::NetMask;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::trace;
use mm1_common::types::Never;
use mm1_core::context::{Call, Fork, ForkErrorKind, Now, Quit, Recv, RecvErrorKind, TellErrorKind};
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_core::message::AnyMessage;
use tokio::time::Instant;

use super::config::{EffectiveActorConfig, Mm1Config};
use crate::runtime::actor_key::ActorKey;
use crate::runtime::mq;
use crate::runtime::rt_api::RtApi;
use crate::runtime::sys_call::{self, SysCall};
use crate::runtime::sys_msg::SysMsg;

mod impl_context_api;

pub struct ActorContext {
    pub(crate) rt_api:             RtApi,
    pub(crate) rt_config:          Arc<Mm1Config>,
    pub(crate) actor_address:      Address,
    pub(crate) rx_priority:        mq::UbRx<Envelope>,
    pub(crate) rx_regular:         mq::Rx<Envelope>,
    pub(crate) tx_system_weak:     mq::UbTxWeak<SysMsg>,
    pub(crate) call:               sys_call::Tx,
    pub(crate) subnet_pool:        SubnetPool,
    pub(crate) actor_key:          ActorKey,
    pub(crate) ack_to:             Option<Address>,
    pub(crate) unregister_on_drop: bool,
}

impl Drop for ActorContext {
    fn drop(&mut self) {
        if self.unregister_on_drop {
            if let Some((address_lease, tx_system)) = self.rt_api.unregister(self.actor_address) {
                let _ = tx_system.send(SysMsg::ForkDone(address_lease));
            }
        }
    }
}

impl Call<Address, AnyMessage> for ActorContext {
    type Outcome = Result<(), ErrorOf<TellErrorKind>>;

    async fn call(&mut self, to: Address, msg: AnyMessage) -> Self::Outcome {
        let info = EnvelopeInfo::new(to);
        let outbound = Envelope::new(info, msg);
        trace!("sending [outbound: {:?}]", outbound);
        self.rt_api
            .send(false, outbound)
            .map_err(|k| ErrorOf::new(k, ""))
    }
}

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

impl Recv for ActorContext {
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
            priority,
            inbound_opt
        );

        inbound_opt.ok_or(ErrorOf::new(RecvErrorKind::Closed, "closed"))
    }

    async fn close(&mut self) {
        self.rx_regular.close();
        self.rx_priority.close();
    }
}
