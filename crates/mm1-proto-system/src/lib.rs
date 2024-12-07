// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

mod link;
mod spawn;
mod start;
mod stop;
mod wait;

pub use link::*;
pub use spawn::*;
pub use start::*;
pub use stop::*;
pub use wait::*;

pub trait System: Copy + Send + 'static {
    type Runnable: Runnable<System = Self>;
}

pub trait Runnable: Send + 'static {
    type System: System<Runnable = Self>;
    fn run_at(&self) -> Self::System;
}
