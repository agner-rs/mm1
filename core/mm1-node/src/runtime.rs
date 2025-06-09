mod container;
mod context;
mod rt;
mod rt_api;
mod sys_call;
mod sys_msg;

pub use context::ActorContext;
pub use rt::Rt;

pub type Local = mm1_runnable::local::BoxedRunnable<ActorContext>;
