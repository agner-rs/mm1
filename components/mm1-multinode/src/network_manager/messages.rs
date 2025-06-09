use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_core::prim::message;

#[message]
pub(super) struct Tick;

#[message]
pub(super) struct SubnetStarted {
    pub(super) net_address: NetAddress,
    pub(super) handled_by:  Address,
}

#[message]
pub(super) struct SubnetStartFailed {
    pub(super) net_address: NetAddress,
}
