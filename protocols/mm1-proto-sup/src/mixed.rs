use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct GetChildRequest<Key> {
    pub child_id: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[message(base_path = ::mm1_proto)]
pub enum GetChildErrorKind {
    UnknownChild,
}

pub type GetChildResponse = Result<Option<Address>, ErrorOf<GetChildErrorKind>>;

impl_error_kind!(GetChildErrorKind);
