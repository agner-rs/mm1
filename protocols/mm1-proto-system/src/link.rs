use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct TrapExit {
    pub enable: bool,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Exited {
    pub peer:        Address,
    pub normal_exit: bool,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Link {
    pub peer: Address,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Unlink {
    pub peer: Address,
}
