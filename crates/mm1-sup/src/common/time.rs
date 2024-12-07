use std::ops::Sub;
use std::time::{Duration, Instant};

pub trait D: Sized + Copy + Ord + Send + Sync + 'static {
    type I: I<D = Self>;
}
pub trait I: Sized + Copy + Sub<Self, Output = Self::D> + Send + Sync + 'static {
    type D: D<I = Self>;
}

impl I for Instant {
    type D = Duration;
}
impl D for Duration {
    type I = Instant;
}

impl I for u32 {
    type D = u32;
}
impl D for u32 {
    type I = u32;
}
