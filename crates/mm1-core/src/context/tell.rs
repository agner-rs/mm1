use std::future::Future;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::Message;

use crate::context::Call;
use crate::message::AnyMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TellErrorKind {
    InternalError,
    NotFound,
    Closed,
    Full,
}

pub trait Tell: Call<Address, AnyMessage, Outcome = Result<(), ErrorOf<TellErrorKind>>> {
    fn tell<M>(
        &mut self,
        to: Address,
        msg: M,
    ) -> impl Future<Output = Result<(), ErrorOf<TellErrorKind>>> + Send
    where
        M: Message,
    {
        async move {
            let message = AnyMessage::new(msg);
            self.call(to, message).await
        }
    }
}

impl<T> Tell for T where T: Call<Address, AnyMessage, Outcome = Result<(), ErrorOf<TellErrorKind>>> {}

impl_error_kind!(TellErrorKind);
