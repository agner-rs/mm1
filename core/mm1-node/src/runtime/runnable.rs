use std::future::Future;

use mm1_common::types::Never;

mod actor_exit;
mod fnonce_actor;

pub use fnonce_actor::{ActorRunBoxed, ActorRunFnOnce};

pub trait ActorRun<Ctx> {
    fn run<'run>(self, context: &'run mut Ctx) -> impl Future<Output = Never> + Send + 'run
    where
        Self: Sized + 'run;
}

pub fn from_fn<F, Ctx>(fn_once: F) -> FnOnceRunnable<F>
where
    F: ActorRunFnOnce<Ctx>,
{
    FnOnceRunnable(fn_once)
}

pub fn boxed_from_fn<Ctx, F>(fn_once: F) -> BoxedRunnable<Ctx>
where
    F: ActorRunBoxed<Ctx> + 'static,
    Ctx: 'static,
{
    BoxedRunnable(Box::new(fn_once), std::any::type_name::<F>())
}

pub struct FnOnceRunnable<F>(F);
pub struct BoxedRunnable<Ctx>(Box<dyn fnonce_actor::ActorRunBoxed<Ctx>>, &'static str);

impl<Ctx> BoxedRunnable<Ctx> {
    pub fn func_name(&self) -> &'static str {
        // self.1.find('<').map(|idx| &self.1[..idx]).unwrap_or(self.1)
        self.1
    }
}

impl<Ctx, F> ActorRun<Ctx> for FnOnceRunnable<F>
where
    F: fnonce_actor::ActorRunFnOnce<Ctx>,
{
    fn run<'run>(self, context: &'run mut Ctx) -> impl Future<Output = Never> + Send + 'run
    where
        Self: Sized + 'run,
    {
        fnonce_actor::ActorRunFnOnce::run(self.0, context)
    }
}

impl<Ctx> ActorRun<Ctx> for BoxedRunnable<Ctx> {
    fn run<'run>(self, context: &'run mut Ctx) -> impl Future<Output = Never> + Send + 'run
    where
        Self: Sized + 'run,
    {
        fnonce_actor::ActorRunBoxed::run(self.0, context)
    }
}

pub trait ActorExit<Ctx>: 'static {
    fn exit(self, context: &mut Ctx) -> impl Future<Output = Never> + Send + '_;
}
