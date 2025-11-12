use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct BindRequest<A> {
    pub bind_address:  A,
    pub protocol_name: crate::ProtocolName,
    pub options:       crate::Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum BindErrorKind {
    DuplicateBindAddr,
}

impl_error_kind!(BindErrorKind);

pub type BindResponse = Result<(), ErrorOf<BindErrorKind>>;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct ConnectRequest<A> {
    pub dst_address:   A,
    pub protocol_name: crate::ProtocolName,
    pub options:       crate::Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum ConnectErrorKind {
    DuplicateDstAddr,
}

impl_error_kind!(ConnectErrorKind);

pub type ConnectResponse = Result<(), ErrorOf<ConnectErrorKind>>;
