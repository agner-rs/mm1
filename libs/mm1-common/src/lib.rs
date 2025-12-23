// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod errors;
#[cfg(feature = "futures")]
pub mod futures;
#[cfg(feature = "logging")]
pub mod log;
pub mod metrics;
pub mod serde;
pub mod types;
