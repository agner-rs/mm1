use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_common::log;
use mm1_common::types::AnyError;
use mm1_core::envelope::Envelope;
use mm1_core::tracing::TraceId;
use tokio::sync::mpsc;

use crate::actor_key::ActorKey;
use crate::config::{Mm1NodeConfig, Valid};
use crate::registry::{ActorNode, MessageWithPermit};
use crate::runtime::rt_api::RtApi;
use crate::runtime::sys_call;
use crate::runtime::sys_msg::SysMsg;

mod impl_context_api;

pub struct ActorContext {
    pub(crate) rt_api:    RtApi,
    pub(crate) rt_config: Arc<Valid<Mm1NodeConfig>>,

    pub(crate) actor_key:     ActorKey,
    pub(crate) address:       Address,
    pub(crate) ack_to:        Option<Address>,
    pub(crate) actor_node:    Weak<ActorNode<(TraceId, SysMsg), Envelope>>,
    pub(crate) network_nodes: BTreeMap<AddressRange, Weak<NetworkNode>>,

    pub(crate) rx_priority:      mpsc::UnboundedReceiver<Envelope>,
    pub(crate) rx_regular:       mpsc::UnboundedReceiver<MessageWithPermit<Envelope>>,
    pub(crate) tx_system_weak:   mpsc::WeakUnboundedSender<(TraceId, SysMsg)>,
    pub(crate) tx_priority_weak: mpsc::WeakUnboundedSender<Envelope>,
    pub(crate) tx_regular_weak:  mpsc::WeakUnboundedSender<MessageWithPermit<Envelope>>,
    pub(crate) call:             sys_call::Tx,

    pub(crate) tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
}

type NetworkNode = crate::registry::NetworkNode<(TraceId, SysMsg), Envelope>;

impl Drop for ActorContext {
    fn drop(&mut self) {
        for (address_range, net_node) in std::mem::take(&mut self.network_nodes) {
            if let Some(_net_node) = net_node.upgrade() {
                let registry = self.rt_api.registry();
                if !registry.unregister(address_range.into()) {
                    log::error!("could not unregister {address_range}");
                }
            }
        }

        if let Some(actor_node) = self.actor_node.upgrade() {
            let fork_lease = actor_node
                .unregister(self.address)
                .expect("already unregistered?");
            let fork_done = SysMsg::ForkDone(fork_lease);
            let _ = actor_node.tx_system.send((TraceId::current(), fork_done));
        }
    }
}
