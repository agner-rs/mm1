pub use mm1_proc_macros::message;

pub trait Message: Send + serde::Serialize + serde::de::DeserializeOwned + 'static {}

impl<M> Message for M where M: Send + serde::Serialize + serde::de::DeserializeOwned + 'static {}
