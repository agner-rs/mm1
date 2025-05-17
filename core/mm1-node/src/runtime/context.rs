use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::pool::Pool as SubnetPool;
use mm1_core::envelope::Envelope;

use super::config::Mm1Config;
use crate::runtime::actor_key::ActorKey;
use crate::runtime::rt_api::RtApi;
use crate::runtime::sys_msg::SysMsg;
use crate::runtime::{mq, sys_call};

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
