use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::Traversable;

use crate::spawn::SpawnErrorKind;

#[derive(Debug, Traversable)]
pub struct InitAck {
    pub address: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Traversable)]
pub enum StartErrorKind {
    Spawn(SpawnErrorKind),
    Exited,
    Timeout,
    InternalError,
}

impl_error_kind!(StartErrorKind);
