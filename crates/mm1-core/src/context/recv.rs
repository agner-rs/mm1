use std::future::Future;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

use crate::envelope::Envelope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum RecvErrorKind {
    Closed,
}

pub trait Recv: Send {
    fn address(&self) -> Address;

    fn recv(&mut self) -> impl Future<Output = Result<Envelope, ErrorOf<RecvErrorKind>>> + Send;

    fn close(&mut self) -> impl Future<Output = ()> + Send;
}

impl_error_kind!(RecvErrorKind);
