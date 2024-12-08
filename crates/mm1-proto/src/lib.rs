// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

mod local;
mod message;
// mod traversable;
mod unique;

pub use local::Local;
pub use message::message;
// pub use traversable::{AnyError, Traversable};
pub use message::Message;
pub use unique::Unique;
