pub mod decider;
pub mod strategy;

mod spec_builder;
mod sup_actor;
mod sup_child;

use std::pin::Pin;
use std::sync::Arc;

pub use sup_actor::{mixed_sup, MixedSupError};

use crate::common::factory::ActorFactory;

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
