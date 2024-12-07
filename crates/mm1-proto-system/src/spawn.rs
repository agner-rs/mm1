use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::Traversable;

use crate::System;

#[derive(Debug, Traversable)]
pub struct SpawnRequest<S: System> {
    pub runnable: S::Runnable,
    pub ack_to:   Option<Address>,
    pub link_to:  Vec<Address>,
}

pub type SpawnResponse = Result<Address, ErrorOf<SpawnErrorKind>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Traversable)]
pub enum SpawnErrorKind {
    InternalError,
    ResourceConstraint,
}

impl_error_kind!(SpawnErrorKind);
