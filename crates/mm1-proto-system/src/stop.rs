use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::Traversable;

#[derive(Debug, Traversable)]
pub struct Exit {
    pub peer: Address,
}

#[derive(Debug, Traversable)]
pub struct Kill {
    pub peer: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Traversable)]
pub enum StopErrorKind {
    NotFound,
    Timeout,
    InternalError,
}

impl_error_kind!(StopErrorKind);
