use std::any::TypeId;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::ControlFlow;

mod behaviour;
mod handler;
mod outcome;

pub use behaviour::{OnMessage, OnRequest};
use eyre::Context;
use mm1_common::log::warn;
use mm1_common::types::AnyError;
use mm1_core::context::Messaging;
use mm1_core::tracing::WithTraceIdExt;
pub use outcome::Outcome;

pub fn new<Ctx>() -> Server<Ctx, (), ()> {
    Server {
        behaviour: (),
        pd:        Default::default(),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Server<Ctx, B, H> {
    behaviour: B,
    pd:        PhantomData<(Ctx, H)>,
}
pub type AppendReq<H, Rq, Rs> = (H, handler::Req<Rq, Rs>);
pub type AppendMsg<H, M> = (H, handler::Msg<M>);

impl<Ctx> Server<Ctx, (), ()> {
    pub fn behaviour<S>(self, behaviour: S) -> Server<Ctx, S, ()> {
        let Self { behaviour: _, pd } = self;
        Server { behaviour, pd }
    }
}

impl<Ctx, B, H> Server<Ctx, B, H> {
    pub fn msg<M>(self) -> Server<Ctx, B, AppendMsg<H, M>>
    where
        B: behaviour::OnMessage<Ctx, M>,
    {
        let Self {
            behaviour: state,
            pd: _,
        } = self;
        Server {
            behaviour: state,
            pd:        Default::default(),
        }
    }

    pub fn req<Rq>(self) -> Server<Ctx, B, AppendReq<H, Rq, B::Rs>>
    where
        B: behaviour::OnRequest<Ctx, Rq>,
    {
        let Self {
            behaviour: state,
            pd: _,
        } = self;
        Server {
            behaviour: state,
            pd:        Default::default(),
        }
    }

    pub async fn run(self, ctx: &mut Ctx) -> Result<B, AnyError>
    where
        Ctx: Messaging,
        H: handler::Register<Ctx, B>,
    {
        let Self { mut behaviour, .. } = self;
        let mut handlers = HashMap::<TypeId, &dyn handler::ErasedHandler<Ctx, B>>::new();

        H::register(&mut handlers);

        loop {
            let envelope = ctx.recv().await.wrap_err("ctx.recv")?;
            let trace_id = envelope.header().trace_id();
            let msg_type_id = envelope.tid();
            let Some(handler) = handlers.get(&msg_type_id) else {
                trace_id.scope_sync(|| warn!(unexpected = ?envelope, "unexpected"));
                continue
            };
            match handler
                .handle(ctx, &mut behaviour, envelope)
                .with_trace_id(trace_id)
                .await
                .wrap_err("handler.handle")?
            {
                ControlFlow::Break(()) => break,
                ControlFlow::Continue(()) => (),
            }
        }

        Ok(behaviour)
    }
}
