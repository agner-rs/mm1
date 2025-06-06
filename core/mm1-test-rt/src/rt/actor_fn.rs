use mm1_common::types::{AnyError, Never};
use mm1_core::context::Quit;

use crate::rt::Context;

pub trait ActorFn<'a, R>: Send + 'a {
    type Fut: Future + Send + 'a;
    type Out;

    fn run(self, context: &'a mut Context<R>) -> Self::Fut;
}

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
        struct AnyErrorWrapped(AnyError);

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
        struct AnyErrorWrapped(AnyError);

        async move {
            match self.map_err(Into::into).map_err(AnyErrorWrapped) {
                Ok(never) => match never {},
                Err(reason) => context.quit_err(reason).await,
            }
        }
    }
}

impl<'a, R, Fun, Fut> ActorFn<'a, R> for Fun
where
    R: 'a,
    Self: Send + 'a,
    Fun: FnOnce(&'a mut Context<R>) -> Fut,
    Fut: Future + Send + 'a,
    Fut::Output: ActorExit<Context<R>>,
{
    type Fut = Fut;
    type Out = Fut::Output;

    fn run(self, context: &'a mut Context<R>) -> Self::Fut {
        (self)(context)
    }
}

macro_rules! impl_actor_func_with_args {
    ( $( $t:ident ),* $(,)? ) => {
        impl<'a, R, Fun, Fut,
                $( $t , )*
            > ActorFn<'a, R> for (Fun, (
                $( $t , )*
            ))
        where
            R: Send + 'a,
            Self: Send + 'a,
            Fun: FnOnce(
                    &'a mut Context<R>,
                    $( $t , )*
                ) -> Fut,
            Fut: Future + Send + 'a,
            Fut::Output: ActorExit<Context<R>>,
        {
            type Fut = Fut;
            type Out = Fut::Output;

            fn run(self, context: &'a mut Context<R>) -> Self::Fut {
                #[allow(non_snake_case)]
                let (f, (
                        $( $t , )*
                    )) = self;
                (f)(
                    context,
                    $( $t , )*
                )
            }
        }
    };
}

impl_actor_func_with_args!(T0);
impl_actor_func_with_args!(T0, T1);
impl_actor_func_with_args!(T0, T1, T2);
impl_actor_func_with_args!(T0, T1, T2, T3);
impl_actor_func_with_args!(T0, T1, T2, T3, T4);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
impl_actor_func_with_args!(
    T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
