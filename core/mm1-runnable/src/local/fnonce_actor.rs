use std::future::Future;
use std::pin::Pin;

use mm1_common::types::Never;
use mm1_core::actor_exit::ActorExit;

pub trait ActorRunBoxed<Ctx>: Send + Sync {
    fn run<'run>(
        self: Box<Self>,
        context: &'run mut Ctx,
    ) -> Pin<Box<dyn Future<Output = Never> + Send + 'run>>
    where
        Self: 'run;
}

pub trait ActorRunFnOnce<Ctx> {
    fn run<'run>(self, context: &'run mut Ctx) -> impl Future<Output = Never> + Send + 'run
    where
        Self: Sized + 'run;
}

pub trait ActorFunc<Ctx> {
    fn call(self, context: &mut Ctx) -> impl Future<Output = impl ActorExit<Ctx>> + Send + '_;
}

pub trait ActorFuncInner<'a, Ctx> {
    type Out: ActorExit<Ctx>;
    type Fut: Future<Output = Self::Out> + Send;

    fn call_inner(self, context: &'a mut Ctx) -> Self::Fut;
}

impl<Ctx, Func> ActorFunc<Ctx> for Func
where
    Func: Send,
    Ctx: Send + Sync,
    for<'a> Func: ActorFuncInner<'a, Ctx> + 'a,
{
    async fn call(self, context: &mut Ctx) -> impl ActorExit<Ctx> {
        self.call_inner(context).await
    }
}

impl<Ctx, Func> ActorRunFnOnce<Ctx> for Func
where
    Func: ActorFunc<Ctx> + Send,
    Ctx: Send + Sync,
{
    async fn run<'run>(self, context: &'run mut Ctx) -> Never
    where
        Self: Sized + 'run,
    {
        self.call(context).await.exit(context).await
    }
}

impl<Ctx, Func> ActorRunBoxed<Ctx> for Func
where
    Func: ActorFunc<Ctx> + Send + Sync,
    Ctx: Send + Sync,
{
    fn run<'run>(
        self: Box<Self>,
        context: &'run mut Ctx,
    ) -> Pin<Box<dyn Future<Output = Never> + Send + 'run>>
    where
        Self: 'run,
    {
        Box::pin(async move { self.call(context).await.exit(context).await })
    }
}

impl<'a, Ctx, Func, Fut> ActorFuncInner<'a, Ctx> for Func
where
    Ctx: 'a,
    Func: FnOnce(&'a mut Ctx) -> Fut,
    Fut: Future + Send + 'a,
    Fut::Output: ActorExit<Ctx>,
{
    type Fut = Fut;
    type Out = Fut::Output;

    fn call_inner(self, context: &'a mut Ctx) -> Self::Fut {
        (self)(context)
    }
}

macro_rules! impl_actor_func_with_args {
    ( $( $t:ident ),* $(,)? ) => {
        impl<'a, Ctx, Func, Fut,
                $( $t , )*
            > ActorFuncInner<'a, Ctx> for (Func, (
                $( $t , )*
            ))
        where
            Ctx: 'a,
            Func: FnOnce(
                    &'a mut Ctx,
                    $( $t , )*
                ) -> Fut,
            Fut: Future + Send + 'a,
            Fut::Output: ActorExit<Ctx>,
        {
            type Fut = Fut;
            type Out = Fut::Output;

            fn call_inner(self, context: &'a mut Ctx) -> Self::Fut {
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
