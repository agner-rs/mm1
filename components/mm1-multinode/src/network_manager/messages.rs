use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_proto::message;

#[message(base_path = ::mm1_proto)]
pub(super) struct Tick;

#[message(base_path = ::mm1_proto)]
pub(super) struct SubnetStarted {
    pub(super) net_address: NetAddress,
    pub(super) handled_by:  Address,
}

#[message(base_path = ::mm1_proto)]
pub(super) struct SubnetStartFailed {
    pub(super) net_address: NetAddress,
}
