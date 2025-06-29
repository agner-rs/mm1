use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[message]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegisterSubnetRequest {
    pub net_address: NetAddress,
    pub config:      serde_json::Value,
}

pub type RegisterSubnetResponse = Result<(), ErrorOf<RegisterSubnetErrorKind>>;

#[message]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RegisterSubnetErrorKind {
    Internal,
    BadRequest,
    Conflict,
}

impl_error_kind!(RegisterSubnetErrorKind);
