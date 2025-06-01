use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug)]
#[message]
pub struct TrapExit {
    pub enable: bool,
}

#[derive(Debug)]
#[message]
pub struct Exited {
    pub peer:        Address,
    pub normal_exit: bool,
}

#[derive(Debug)]
#[message]
pub struct Link {
    pub peer: Address,
}

#[derive(Debug)]
#[message]
pub struct Unlink {
    pub peer: Address,
}
