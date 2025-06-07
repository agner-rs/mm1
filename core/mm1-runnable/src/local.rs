mod fnonce_actor;
mod runnable;

// pub use fnonce_actor::ActorRunBoxed;
// pub use fnonce_actor::ActorRunFnOnce;
pub use runnable::{ActorRun, BoxedRunnable, boxed_from_fn, from_fn};
