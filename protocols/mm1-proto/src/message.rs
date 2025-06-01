pub use mm1_proc_macros::message;

pub trait Message: Send + 'static {}

impl<M> Message for M where M: Send + 'static {}
