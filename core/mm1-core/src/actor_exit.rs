use mm1_common::types::{AnyError, Never};

use crate::context::Quit;

pub trait ActorExit<Ctx>: 'static {
    fn exit(self, context: &mut Ctx) -> impl Future<Output = Never> + Send + '_;
}

impl<Ctx> ActorExit<Ctx> for Never
where
    Ctx: Send,
{
    async fn exit(self, _context: &mut Ctx) -> Never {
        match self {}
    }
}

impl<Ctx> ActorExit<Ctx> for ()
where
    Ctx: Quit,
{
    fn exit(self, context: &mut Ctx) -> impl Future<Output = Never> + '_ {
        context.quit_ok()
    }
}

impl<Ctx, E> ActorExit<Ctx> for Result<(), E>
where
    E: Into<AnyError> + Send + Sync + 'static,
    Ctx: Quit,
{
    fn exit(self, context: &mut Ctx) -> impl Future<Output = Never> + Send + '_ {
        #[derive(Debug, thiserror::Error)]
        #[error("wrapped dyn-error: {}", _0)]
        struct AnyErrorWrapped(#[source] AnyError);

        async move {
            match self.map_err(Into::into).map_err(AnyErrorWrapped) {
                Ok(()) => context.quit_ok().await,
                Err(reason) => context.quit_err(reason).await,
            }
        }
    }
}

impl<Ctx, E> ActorExit<Ctx> for Result<Never, E>
where
    E: Into<AnyError> + Send + Sync + 'static,
    Ctx: Quit,
{
    fn exit(self, context: &mut Ctx) -> impl Future<Output = Never> + Send + '_ {
        #[derive(Debug, thiserror::Error)]
        #[error("wrapped dyn-error: {}", _0)]
        struct AnyErrorWrapped(#[source] AnyError);

        async move {
            match self.map_err(Into::into).map_err(AnyErrorWrapped) {
                Ok(never) => match never {},
                Err(reason) => context.quit_err(reason).await,
            }
        }
    }
}
