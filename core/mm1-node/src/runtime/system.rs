use super::runnable::BoxedRunnable;
use crate::runtime::context::ActorContext;
use crate::runtime::runnable;

impl BoxedRunnable<ActorContext> {
    pub fn actor<F>(actor: F) -> Self
    where
        F: runnable::ActorRunBoxed<ActorContext> + 'static,
    {
        runnable::boxed_from_fn(actor)
    }
}
