use mm1_address::address::Address;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Exit {
    pub peer: Address,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Kill {
    pub peer: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum StopErrorKind {
    NotFound,
    Timeout,
    InternalError,
}

impl_error_kind!(StopErrorKind);
