use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::{Lease, Pool};
use mm1_address::subnet::NetAddress;
use mm1_common::log;
use mm1_common::types::AnyError;
use mm1_core::envelope::Envelope;
use mm1_core::tracing::TraceId;
use tokio::sync::{Notify, mpsc};

use crate::actor_key::ActorKey;
use crate::config::{Mm1NodeConfig, Valid};
use crate::registry::{MessageWithPermit, MessageWithoutPermit, WeakSubnetMailboxTx};
use crate::runtime::rt_api::RtApi;
use crate::runtime::sys_call;
use crate::runtime::sys_msg::SysMsg;

mod impl_context_api;

pub struct ActorContext {
    pub(crate) fork_address: Address,
    pub(crate) fork_lease:   Option<Lease>,
    pub(crate) ack_to:       Option<Address>,
    pub(crate) call:         sys_call::Tx,

    pub(crate) subnet_context: Arc<spin::lock_api::Mutex<SubnetContext>>,
}

#[derive(Default)]
pub(crate) struct ForkEntry {
    fork_notifiy:   Arc<Notify>,
    inbox_priority: VecDeque<MessageWithoutPermit<Envelope>>,
    inbox_regular:  VecDeque<MessageWithPermit<Envelope>>,
}

pub(crate) struct SubnetContext {
    pub(crate) rt_api:    RtApi,
    pub(crate) rt_config: Arc<Valid<Mm1NodeConfig>>,

    pub(crate) actor_key:      ActorKey,
    pub(crate) subnet_pool:    Pool,
    pub(crate) subnet_address: NetAddress,

    pub(crate) subnet_mailbox_tx: WeakSubnetMailboxTx<(TraceId, SysMsg), Envelope>,

    pub(crate) subnet_notify: Arc<Notify>,
    pub(crate) rx_priority:   kanal::Receiver<MessageWithoutPermit<Envelope>>,
    pub(crate) rx_regular:    kanal::Receiver<MessageWithPermit<Envelope>>,

    pub(crate) fork_entries:  HashMap<Address, ForkEntry>,
    pub(crate) bound_subnets: BTreeMap<AddressRange, Address>,

    // TODO: send the NetAddress instead of Address here
    pub(crate) tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
}

impl Drop for ActorContext {
    fn drop(&mut self) {
        let Self {
            fork_address,
            subnet_context,
            ..
        } = self;
        let mut subnet_context_locked = subnet_context
            .try_lock()
            .expect("could not lock subnet_context");
        let SubnetContext {
            rt_api,
            subnet_address,
            subnet_mailbox_tx,
            fork_entries,
            bound_subnets,
            ..
        } = &mut *subnet_context_locked;

        let registry = rt_api.registry();

        let mut unbound_subnets = vec![];
        for (address_range, bound_by) in bound_subnets.iter() {
            if bound_by == fork_address {
                unbound_subnets.push(*address_range);
            }
        }

        for address_range in unbound_subnets {
            bound_subnets.remove(&address_range);
            if !registry.unregister(address_range.into()) {
                log::error!(%address_range, "could not unregister bound subnet");
            }
        }
        if fork_entries.remove(fork_address).is_none() {
            log::error!(
                fork = %fork_address,
                "nothing actually removed from fork_entries"
            );
        }

        let fork_lease_opt = self.fork_lease.take();
        let subnet_system_tx_opt = subnet_mailbox_tx.tx_system.upgrade();

        if let Some((fork_lease, system_tx)) = fork_lease_opt.zip(subnet_system_tx_opt) {
            let fork_done = SysMsg::ForkDone(fork_lease);
            let _ = system_tx.send((TraceId::current(), fork_done));
        }

        if fork_entries.is_empty() {
            let _ = registry.unregister(*subnet_address);
        }
    }
}
