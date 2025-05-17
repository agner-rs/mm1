use std::future::Future;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TellErrorKind {
    InternalError,
    NotFound,
    Closed,
    Full,
}

pub trait Tell {
    fn tell<M>(
        &mut self,
        to: Address,
        msg: M,
    ) -> impl Future<Output = Result<(), ErrorOf<TellErrorKind>>> + Send
    where
        M: Message;
}

impl_error_kind!(TellErrorKind);
