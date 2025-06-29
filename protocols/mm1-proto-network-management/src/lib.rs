use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[message(base_path = ::mm1_proto)]
pub struct RegisterSubnetRequest {
    pub net_address: NetAddress,
    pub config:      serde_json::Value,
}

pub type RegisterSubnetResponse = Result<(), ErrorOf<RegisterSubnetErrorKind>>;

#[message(base_path = ::mm1_proto)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegisterSubnetErrorKind {
    Internal,
    BadRequest,
    Conflict,
}

impl_error_kind!(RegisterSubnetErrorKind);
