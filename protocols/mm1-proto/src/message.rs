pub use mm1_proc_macros::message;
use serde::Serialize;

mod primitives;

pub trait Message: Serialize + Send + 'static {}
