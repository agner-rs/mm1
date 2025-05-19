use std::future::Future;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::{Message, message};

use super::{Fork, ForkErrorKind};
use crate::envelope::{Envelope, EnvelopeHeader};

#[derive(Debug, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum RecvErrorKind {
    Closed,
}

impl_error_kind!(RecvErrorKind);

#[derive(Debug, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum SendErrorKind {
    InternalError,
    NotFound,
    Closed,
    Full,
}

pub trait Messaging {
    fn address(&self) -> Address;

    fn recv(&mut self) -> impl Future<Output = Result<Envelope, ErrorOf<RecvErrorKind>>> + Send;

    fn close(&mut self) -> impl Future<Output = ()> + Send;

    fn send(
        &mut self,
        envelope: Envelope,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send;
}

impl_error_kind!(SendErrorKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AskErrorKind {
    Fork(ForkErrorKind),
    Tell(SendErrorKind),
    Recv(RecvErrorKind),
}

pub trait Ask: Messaging + Fork {
    fn ask<Req>(
        &mut self,
        to: Address,
        make_request: impl FnOnce(Address) -> Req + Send,
    ) -> impl Future<Output = Result<Envelope, ErrorOf<AskErrorKind>>> + Send
    where
        Req: Message,
    {
        async move {
            let mut forked = self
                .fork()
                .await
                .map_err(|e| e.map_kind(AskErrorKind::Fork))?;

            let reply_to = forked.address();
            let request = make_request(reply_to);
            self.tell(to, request)
                .await
                .map_err(|e| e.map_kind(AskErrorKind::Tell))?;

            let inbound = forked
                .recv()
                .await
                .map_err(|e| e.map_kind(AskErrorKind::Recv))?;

            Ok(inbound)
        }
    }
}

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

impl_error_kind!(AskErrorKind);

impl<T> Ask for T where T: Messaging + Fork {}
impl<T> Tell for T where T: Messaging {}
