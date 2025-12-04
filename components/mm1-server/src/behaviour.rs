use futures::never::Never;
use mm1_common::types::AnyError;
use mm1_proto_ask::RequestHeader;

pub enum Outcome<Rs = Never> {
    Reply(Rs),
    NoReply,
    Break,
}

pub trait OnMessage<Ctx, M>: Send {
    fn on_message(
        &mut self,
        ctx: &mut Ctx,
        message: M,
    ) -> impl Future<Output = Result<Outcome, AnyError>> + Send;
}

pub trait OnRequest<Ctx, Rq>: Send {
    type Rs;

    fn on_request(
        &mut self,
        ctx: &mut Ctx,
        reply_to: RequestHeader,
        request: Rq,
    ) -> impl Future<Output = Result<Outcome<Self::Rs>, AnyError>> + Send;
}
