use std::future::Future;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::Message;

use crate::context::{Fork, ForkErrorKind, Recv, RecvErrorKind, Tell, TellErrorKind};
use crate::envelope::Envelope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AskErrorKind {
    Fork(ForkErrorKind),
    Tell(TellErrorKind),
    Recv(RecvErrorKind),
}

pub trait Ask: Tell + Fork + Recv {
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

impl_error_kind!(AskErrorKind);

impl<T> Ask for T where T: Tell + Fork + Recv {}
