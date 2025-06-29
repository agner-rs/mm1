use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug)]
#[message]
pub struct Exit {
    pub peer: Address,
}

#[derive(Debug)]
#[message]
pub struct Kill {
    pub peer: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum StopErrorKind {
    NotFound,
    Timeout,
    InternalError,
}

impl_error_kind!(StopErrorKind);
