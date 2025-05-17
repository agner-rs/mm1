pub use mm1_proc_macros::message;

pub trait Message: Clone + Send + 'static {}

impl<M> Message for M where M: Clone + Send + 'static {}
