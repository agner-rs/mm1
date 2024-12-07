pub mod decider;

mod sup_actor;

pub use sup_actor::main;

#[derive(Clone)]
pub struct MixedSup<RS, C> {
    pub restart_strategy: RS,
    pub children:         C,
}
