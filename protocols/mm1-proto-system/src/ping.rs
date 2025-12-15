use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug, Clone, Copy)]
#[message(base_path = ::mm1_proto)]
pub struct Ping {
    pub reply_to: Option<Address>,
    pub id:       u64,
}
