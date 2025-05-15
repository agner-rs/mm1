use std::marker::PhantomData;

use crate::common::restart_intensity::RestartIntensity;
use crate::mixed::decider::Decider;

mod error;
pub use error::DeciderError;

mod one_for_one;

pub trait RestartStrategy<Key>: Clone {
    type Decider: Decider<Key = Key>;

    fn decider(&self) -> Self::Decider;
}

#[derive(Debug)]
pub struct OneForOne<Key> {
    pub restart_intensity: RestartIntensity,
    _pd:                   PhantomData<Key>,
}

#[derive(Debug)]
pub struct AllForOne<Key> {
    pub restart_intensity: RestartIntensity,
    _pd:                   PhantomData<Key>,
}
