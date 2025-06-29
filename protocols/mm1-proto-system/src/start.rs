use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum SpawnErrorKind {
    InternalError,
    ResourceConstraint,
}

impl_error_kind!(SpawnErrorKind);

#[derive(Debug)]
#[message]
pub struct InitAck {
    pub address: Address,
}

#[derive(Debug, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum StartErrorKind {
    Spawn(SpawnErrorKind),
    Exited,
    Timeout,
    InternalError,
}

impl_error_kind!(StartErrorKind);
