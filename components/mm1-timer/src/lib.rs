// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod actor;
pub mod api;

#[cfg(feature = "tokio-time")]
pub mod tokio_time;

#[cfg(feature = "tokio-time")]
pub use tokio_time::new_tokio_timer;
