use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_proto::Traversable;
use mm1_proto_system::{StartErrorKind, StopErrorKind};

#[derive(Debug, Traversable)]
pub struct StartRequest<Args> {
    pub reply_to: Address,
    pub args:     Args,
}

pub type StartResponse = Result<Address, ErrorOf<StartErrorKind>>;

pub struct StopRequest {
    pub reply_to: Address,
    pub child:    Address,
}

pub type StopResponse = Result<(), ErrorOf<StopErrorKind>>;
