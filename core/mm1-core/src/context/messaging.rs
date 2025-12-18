use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::{Message, message};

use crate::envelope::{Envelope, EnvelopeHeader};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum RecvErrorKind {
    Closed,
}

impl_error_kind!(RecvErrorKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum SendErrorKind {
    InternalError,
    NotFound,
    Closed,
    Full,
    TtlExhausted,
}

pub trait Messaging {
    fn address(&self) -> Address;

    fn recv(&mut self) -> impl Future<Output = Result<Envelope, ErrorOf<RecvErrorKind>>> + Send;

    fn close(&mut self) -> impl Future<Output = ()> + Send;

    fn send(
        &mut self,
        envelope: Envelope,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send;

    fn forward(
        &mut self,
        to: Address,
        envelope: Envelope,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send;
}

impl_error_kind!(SendErrorKind);

pub trait Tell: Messaging {
    fn tell<M>(
        &mut self,
        to: Address,
        message: M,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send
    where
        M: Message,
    {
        let info = EnvelopeHeader::to_address(to);
        let envelope = Envelope::new(info, message);
        self.send(envelope.into_erased())
    }
}

impl<T> Tell for T where T: Messaging {}
