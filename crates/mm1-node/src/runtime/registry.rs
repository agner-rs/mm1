use std::ops::Deref;

use mm1_address::address::Address;
use mm1_address::pool::Lease;
use mm1_core::envelope::Envelope;

use crate::runtime::mq;
use crate::runtime::sys_msg::SysMsg;

#[derive(Debug)]
pub(crate) struct Registry(scc::HashMap<Address, Entry>);

#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) address_lease: Lease,
    pub(crate) tx_system:     mq::UbTx<SysMsg>,
    pub(crate) tx_priority:   mq::UbTx<Envelope>,
    pub(crate) tx_regular:    mq::Tx<Envelope>,
}

impl Deref for Registry {
    type Target = scc::HashMap<Address, Entry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Registry {
    pub(crate) fn new() -> Self {
        Self(scc::HashMap::new())
    }
}
