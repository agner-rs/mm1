use mm1_address::address::Address;
use mm1_proto::Traversable;

#[derive(Debug, Traversable)]
pub struct TrapExit {
    pub enable: bool,
}

#[derive(Debug, Traversable)]
pub struct Exited {
    pub peer:        Address,
    pub normal_exit: bool,
}

#[derive(Debug, Traversable)]
pub struct Link {
    pub peer: Address,
}

pub struct Unlink {
    pub peer: Address,
}
