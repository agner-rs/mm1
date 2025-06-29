use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_proto::message;
use mm1_proto_system::{StartErrorKind, StopErrorKind};

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct StartRequest<Args> {
    #[serde(skip)]
    pub args: Args,
}

pub type StartResponse = Result<Address, ErrorOf<StartErrorKind>>;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct StopRequest {
    pub child: Address,
}

pub type StopResponse = Result<(), ErrorOf<StopErrorKind>>;
