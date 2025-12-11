use std::any::TypeId;
use std::collections::HashMap;
use std::collections::hash_map::Entry::*;
use std::ops::ControlFlow;

use eyre::Context;
use futures::FutureExt;
use futures::future::BoxFuture;
use mm1_ask::Reply;
use mm1_common::log::{Instrument, error};
use mm1_common::make_metrics;
use mm1_common::metrics::MeasuredFutureExt;
use mm1_common::types::AnyError;
use mm1_core::envelope::Envelope;
use mm1_proto::Message;
use mm1_proto_ask::{Request, RequestHeader, Response};
use tracing::Level;

use crate::behaviour::{OnMessage, OnRequest, Outcome};

pub trait Register<Ctx, B> {
    fn register(handlers: &mut HashMap<TypeId, &dyn ErasedHandler<Ctx, B>>);
}

pub trait ErasedHandler<Ctx, B>: Sync {
    fn handle<'a>(
        &self,
        ctx: &'a mut Ctx,
        behaviour: &'a mut B,
        envelope: Envelope,
    ) -> BoxFuture<'a, Result<ControlFlow<()>, AnyError>>;
}

#[derive(Debug, Clone, Copy)]
#[ghost::phantom]
pub struct Msg<M>;

#[derive(Debug, Clone, Copy)]
#[ghost::phantom]
pub struct Req<Rq, Rs>;

impl<Ctx, B, L, R> Register<Ctx, B> for (L, R)
where
    L: Register<Ctx, B>,
    R: Register<Ctx, B>,
{
    fn register(handlers: &mut HashMap<TypeId, &dyn ErasedHandler<Ctx, B>>) {
        L::register(handlers);
        R::register(handlers);
    }
}

impl<Ctx, B> Register<Ctx, B> for () {
    fn register(_handlers: &mut HashMap<TypeId, &dyn ErasedHandler<Ctx, B>>) {}
}

impl<Ctx, B, M> Register<Ctx, B> for Msg<M>
where
    B: OnMessage<Ctx, M>,
    Ctx: Send,
    M: Message + Sync,
{
    fn register(handlers: &mut HashMap<TypeId, &dyn ErasedHandler<Ctx, B>>) {
        let Vacant(v) = handlers.entry(TypeId::of::<M>()) else {
            error!(message_type = %std::any::type_name::<M>(), "duplicate handler");
            return
        };
        v.insert(&Msg::<M>);
    }
}

impl<Ctx, B, Rq> Register<Ctx, B> for Req<Rq, B::Rs>
where
    B: OnRequest<Ctx, Rq>,
    Ctx: Reply + Send,
    Rq: Message + Sync,
    Rq: 'static,
    B::Rs: Message + Send + Sync,
{
    fn register(handlers: &mut HashMap<TypeId, &dyn ErasedHandler<Ctx, B>>) {
        let Vacant(v) = handlers.entry(TypeId::of::<Request<Rq>>()) else {
            error!(
                request_type = %std::any::type_name::<Rq>(),
                message_type = %std::any::type_name::<Request<Rq>>(),
                "duplicate handler"
            );
            return;
        };

        v.insert(&Req::<Rq, B::Rs>);
    }
}

impl<Ctx, B, M> ErasedHandler<Ctx, B> for Msg<M>
where
    B: OnMessage<Ctx, M>,
    Ctx: Send,
    M: Message + Sync,
{
    fn handle<'a>(
        &self,
        ctx: &'a mut Ctx,
        behaviour: &'a mut B,
        envelope: Envelope,
    ) -> BoxFuture<'a, Result<ControlFlow<()>, AnyError>> {
        async move {
            let envelope = envelope
                .cast::<M>()
                .expect("should not have dispatched here");
            let (message, _empty_envelope) = envelope.take();
            let outcome = behaviour.on_message(ctx, message).await.wrap_err_with(|| {
                eyre::format_err!(
                    "{} as OnMessage<{}>",
                    std::any::type_name::<B>(),
                    std::any::type_name::<M>()
                )
            })?;
            let c = match outcome {
                Outcome::Break => ControlFlow::Break(()),
                Outcome::NoReply => ControlFlow::Continue(()),
            };
            Ok(c)
        }
        .measured(make_metrics!("mm1_server_on_message",
            "beh" => std::any::type_name::<B>(),
            "msg" => std::any::type_name::<M>(),
        ))
        .instrument(tracing::span!(
            Level::TRACE,
            "mm1_server_on_message",
            beh = std::any::type_name::<B>(),
            msg = std::any::type_name::<M>(),
        ))
        .boxed()
    }
}
impl<Ctx, B, Rq, Rs> ErasedHandler<Ctx, B> for Req<Rq, Rs>
where
    B: OnRequest<Ctx, Rq, Rs = Rs>,
    Ctx: Reply + Send,
    Request<Rq>: Message,
    Rq: Sync,
    Response<Rs>: Message,
    Rs: Send + Sync,
{
    fn handle<'a>(
        &self,
        ctx: &'a mut Ctx,
        behaviour: &'a mut B,
        envelope: Envelope,
    ) -> BoxFuture<'a, Result<ControlFlow<()>, AnyError>> {
        async move {
            let envelope = envelope
                .cast::<Request<Rq>>()
                .expect("should not have dispatched here");
            let (
                Request {
                    header: request_header,
                    payload: request,
                },
                _empty_envelope,
            ) = envelope.take();
            let RequestHeader { id, reply_to } = request_header;
            let outcome = behaviour
                .on_request(ctx, RequestHeader { id, reply_to }, request)
                .await
                .wrap_err_with(|| {
                    format!(
                        "{} as OnRequest<{}>",
                        std::any::type_name::<B>(),
                        std::any::type_name::<Rq>()
                    )
                })?;
            let c = match outcome {
                Outcome::Break => ControlFlow::Break(()),
                Outcome::NoReply => ControlFlow::Continue(()),
                Outcome::Reply(reply_with) => {
                    ctx.reply(RequestHeader { id, reply_to }, reply_with)
                        .await
                        .ok();
                    ControlFlow::Continue(())
                },
            };
            Ok(c)
        }
        .measured(make_metrics!("mm1_server_on_request",
            "beh" => std::any::type_name::<B>(),
            "req" => std::any::type_name::<Rq>(),
            "resp" => std::any::type_name::<Rs>(),
        ))
        .instrument(tracing::span!(
            Level::TRACE,
            "mm1_server_on_request",
            beh = std::any::type_name::<B>(),
            req = std::any::type_name::<Rq>(),
            resp = std::any::type_name::<Rs>(),
        ))
        .boxed()
    }
}
