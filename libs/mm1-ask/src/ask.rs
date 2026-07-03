use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_kind::ErrorKind;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_common::log::{Instrument, trace};
use mm1_common::metrics::MeasuredFutureExt;
use mm1_common::{impl_error_kind, make_metrics};
use mm1_core::context::{Fork, ForkErrorKind, Messaging, RecvErrorKind, SendErrorKind};
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_proto::Message;
use mm1_proto_ask::{Request, RequestHeader, Response, ResponseHeader};
use tracing::Level;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

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
        Self: Fork,
        Rq: Send,
        Request<Rq>: Message,
        Rs: Message;

    #[doc(hidden)]
    fn ask_nofork<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> impl Future<Output = Result<Rs, ErrorOf<AskErrorKind>>> + Send
    where
        Rq: Send,
        Request<Rq>: Message,
        Rs: Message;
}

pub trait Reply: Messaging + Send {
    fn reply<Rs>(
        &mut self,
        to: RequestHeader,
        response: Rs,
    ) -> impl Future<Output = Result<(), ErrorOf<SendErrorKind>>> + Send
    where
        Rs: Send,
        Response<Rs>: Message;
}

impl<Ctx> Ask for Ctx
where
    Ctx: Messaging + Sized + Send,
{
    async fn ask_nofork<Rq, Rs>(
        &mut self,
        server: Address,
        request: Rq,
        timeout: Duration,
    ) -> Result<Rs, ErrorOf<AskErrorKind>>
    where
        Request<Rq>: Message,
        Response<Rs>: Message,
    {
        ask_impl(self, server, request, timeout)
            .measured(make_metrics!("mm1_ask",
                "req" => std::any::type_name::<Rq>(),
                "resp" => std::any::type_name::<Rs>(),
            ))
            .instrument(tracing::span!(
                Level::TRACE,
                "mm1_ask",
                req = std::any::type_name::<Rq>(),
                resp = std::any::type_name::<Rs>(),
            ))
            .await
    }

    async fn ask<Rq, Rs>(
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
            .ask_nofork(server, request, timeout)
            .await
    }
}

impl<Ctx> Reply for Ctx
where
    Ctx: Messaging + Send,
{
    async fn reply<Rs>(
        &mut self,
        to: RequestHeader,
        response: Rs,
    ) -> Result<(), ErrorOf<SendErrorKind>>
    where
        Response<Rs>: Message,
    {
        let RequestHeader { id, reply_to } = to;
        let response_header = ResponseHeader { id };
        let response_message = Response {
            header:  response_header,
            payload: response,
        };
        let response_envelope_header = EnvelopeHeader::to_address(reply_to).with_priority(true);
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

async fn ask_impl<Ctx, Rq, Rs>(
    ctx: &mut Ctx,
    server: Address,
    request: Rq,
    timeout: Duration,
) -> Result<Rs, ErrorOf<AskErrorKind>>
where
    Ctx: Messaging,
    Request<Rq>: Message,
    Response<Rs>: Message,
{
    let reply_to = ctx.address();
    let request_id = REQUEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
    let request_message = Request {
        header:  RequestHeader {
            id: request_id,
            reply_to,
        },
        payload: request,
    };
    let request_envelope = Envelope::new(EnvelopeHeader::to_address(server), request_message);
    let () = ctx
        .send(request_envelope.into_erased())
        .await
        .map_err(into_ask_error)?;

    // Messages that are not our response are set aside and put back on the
    // mailbox afterwards, so an in-flight ask never destroys an unrelated
    // message (#136). This lives outside the timed loop, so a timeout keeps
    // them too.
    let mut bystanders: Vec<Envelope> = Vec::new();

    let outcome = {
        let ctx = &mut *ctx;
        let bystanders = &mut bystanders;
        async move {
            loop {
                let envelope = ctx.recv().await.map_err(into_ask_error)?;
                match envelope.cast::<Response<Rs>>() {
                    Ok(response_envelope) => {
                        let (
                            Response {
                                header: ResponseHeader { id },
                                payload,
                            },
                            _empty,
                        ) = response_envelope.take();
                        if id == request_id {
                            return Ok(payload)
                        }
                        // A response to an earlier, already-abandoned request:
                        // drop it so it is never returned as this ask's answer.
                        trace!(got = id, want = request_id, "discarding stale response");
                    },
                    Err(other) => {
                        // Not our response — an unrelated message. Keep it to put
                        // back for the actor's normal receive loop.
                        bystanders.push(other);
                    },
                }
            }
        }
        .timeout(timeout)
        .await
    };

    // Put unrelated messages back, whether the ask succeeded or timed out.
    for bystander in bystanders.drain(..) {
        let _ = ctx.forward(reply_to, bystander).await;
    }

    match outcome {
        Ok(result) => result,
        Err(_elapsed) => {
            Err(ErrorOf::new(
                AskErrorKind::Timeout,
                "timed out waiting for response",
            ))
        },
    }
}
