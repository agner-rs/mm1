use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum SpawnErrorKind {
    InternalError,
    ResourceConstraint,
}

impl_error_kind!(SpawnErrorKind);

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct InitAck {
    pub address: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum StartErrorKind {
    Spawn(SpawnErrorKind),
    Exited,
    Timeout,
    InternalError,
}

impl_error_kind!(StartErrorKind);
