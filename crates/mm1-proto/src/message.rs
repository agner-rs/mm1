pub use mm1_proc_macros::message;

pub trait Message: Clone + Send + serde::Serialize + serde::de::DeserializeOwned + 'static {}

impl<M> Message for M where
    M: Clone + Send + serde::Serialize + serde::de::DeserializeOwned + 'static
{
}
