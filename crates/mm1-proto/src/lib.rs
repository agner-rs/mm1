// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

mod local;
mod traversable;
mod unique;

pub use local::Local;
pub use traversable::{AnyError, Traversable};
pub use unique::Unique;
