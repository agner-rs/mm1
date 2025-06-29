use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_proto::message;
use mm1_proto_system::{StartErrorKind, StopErrorKind};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug)]
#[message]
pub struct StartRequest<Args> {
    pub args: Args,
}

pub type StartResponse = Result<Address, ErrorOf<StartErrorKind>>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug)]
#[message]
pub struct StopRequest {
    pub child: Address,
}

pub type StopResponse = Result<(), ErrorOf<StopErrorKind>>;
