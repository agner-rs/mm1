use futures::never::Never;
use mm1_common::types::AnyError;
use mm1_proto_ask::RequestHeader;

use crate::outcome::Outcome;

pub trait OnMessage<Ctx, M>: Send {
    fn on_message(
        &mut self,
        ctx: &mut Ctx,
        message: M,
    ) -> impl Future<Output = Result<Outcome<M, Never>, AnyError>> + Send;
}

pub trait OnRequest<Ctx, Rq>: Send {
    type Rs;

    fn on_request(
        &mut self,
        ctx: &mut Ctx,
        reply_to: RequestHeader,
        request: Rq,
    ) -> impl Future<Output = Result<Outcome<Rq, Self::Rs>, AnyError>> + Send;
}
