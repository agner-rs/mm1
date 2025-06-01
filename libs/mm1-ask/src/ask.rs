use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_kind::ErrorKind;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_common::impl_error_kind;
use mm1_core::context::{Fork, ForkErrorKind, Messaging, RecvErrorKind, SendErrorKind};
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_core::prim::Message;
use mm1_proto_ask::{Request, RequestHeader, Response, ResponseHeader};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::From)]
pub enum AskErrorKind {
    Send(SendErrorKind),
    Recv(RecvErrorKind),
    Fork(ForkErrorKind),
    Timeout,
    Cast,
}

pub trait Ask: Messaging + Sized {
    fn ask<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> impl Future<Output = Result<Rs, ErrorOf<AskErrorKind>>> + Send
    where
        Rq: Send,
        Request<Rq>: Message,
        Rs: Message;

    fn fork_ask<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> impl Future<Output = Result<Rs, ErrorOf<AskErrorKind>>> + Send
    where
        Self: Fork,
        Rq: Send,
        Request<Rq>: Message,
        Rs: Message;
}

pub trait Reply: Messaging + Send {
    fn reply<Id, Rs>(
        &mut self,
        to: RequestHeader<Id>,
        response: Rs,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send
    where
        Id: Send,
        Rs: Send,
        Response<Rs, Id>: Message;
}

impl<Ctx> Ask for Ctx
where
    Ctx: Messaging + Sized + Send,
{
    async fn ask<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> Result<Rs, ErrorOf<AskErrorKind>>
    where
        Request<Rq>: Message,
        Rs: Message,
    {
        let reply_to = self.address();
        let request_header = RequestHeader { id: (), reply_to };
        let request_message = Request {
            header:  request_header,
            payload: request,
        };
        let request_header = EnvelopeHeader::to_address(server);
        let request_envelope = Envelope::new(request_header, request_message);
        let () = self
            .send(request_envelope.into_erased())
            .await
            .map_err(into_ask_error)?;
        let response_envelope = self
            .recv()
            .timeout(timeout)
            .await
            .map_err(|_elapsed| {
                ErrorOf::new(AskErrorKind::Timeout, "timed out waiting for response")
            })?
            .map_err(into_ask_error)?
            .cast::<Response<Rs>>()
            .map_err(|_| ErrorOf::new(AskErrorKind::Cast, "unexpected response type"))?;
        let (response_message, _empty_envelope) = response_envelope.take();
        let Response {
            header: _,
            payload: response,
        } = response_message;

        Ok(response)
    }

    async fn fork_ask<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> Result<Rs, ErrorOf<AskErrorKind>>
    where
        Self: Fork,
        Rq: Send,
        Request<Rq>: Message,
        Rs: Message,
    {
        self.fork()
            .await
            .map_err(into_ask_error)?
            .ask(server, request, timeout)
            .await
    }
}

impl<Ctx> Reply for Ctx
where
    Ctx: Messaging + Send,
{
    async fn reply<Id, Rs>(
        &mut self,
        to: RequestHeader<Id>,
        response: Rs,
    ) -> Result<(), ErrorOf<SendErrorKind>>
    where
        Response<Rs, Id>: Message,
    {
        let RequestHeader { id, reply_to } = to;
        let response_header = ResponseHeader { id };
        let response_message = Response {
            header:  response_header,
            payload: response,
        };
        let response_envelope_header = EnvelopeHeader::to_address(reply_to);
        let response_envelope = Envelope::new(response_envelope_header, response_message);
        self.send(response_envelope.into_erased()).await?;

        Ok(())
    }
}

impl_error_kind!(AskErrorKind);

fn into_ask_error<K>(e: ErrorOf<K>) -> ErrorOf<AskErrorKind>
where
    K: ErrorKind + Into<AskErrorKind>,
{
    e.map_kind(Into::into)
}
