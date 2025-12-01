pub mod decider;
pub mod strategy;

mod spec_builder;
mod sup_actor;
mod sup_child;

use std::pin::Pin;
use std::sync::Arc;

use mm1_core::context::{Fork, InitDone, Linking, Messaging, Now, Quit, Start, Stop, Watching};
pub use sup_actor::{MixedSupError, mixed_sup};

use crate::common::factory::ActorFactory;

pub trait MixedSupContext<Runnable>:
    Fork
    + InitDone
    + Linking
    + Messaging
    + Now<Instant = tokio::time::Instant>
    + Quit
    + Start<Runnable>
    + Stop
    + Watching
{
}
impl<Ctx, Runnable> MixedSupContext<Runnable> for Ctx where
    Ctx: Fork
        + InitDone
        + Linking
        + Messaging
        + Now<Instant = tokio::time::Instant>
        + Quit
        + Start<Runnable>
        + Stop
        + Watching
{
}

#[derive(Debug, Clone, Copy)]
pub enum ChildType {
    Permanent,
    Transient,
    Temporary,
}

pub struct MixedSup<RS, C> {
    restart_strategy: RS,
    children:         C,
}

pub type ErasedActorFactory<R> = Pin<Arc<dyn ActorFactory<Args = (), Runnable = R>>>;

impl<RS> MixedSup<RS, ()> {
    pub fn new(restart_strategy: RS) -> Self {
        Self {
            restart_strategy,
            children: (),
        }
    }
}
