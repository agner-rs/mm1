use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug)]
#[message[base_path = ::mm1_proto]]
pub struct SetParent {
    pub parent: Address,
}
