pub use std::error::Error as StdError;

pub trait AnError: StdError + Send + Sync + 'static {}

impl<E> AnError for E where E: StdError + Send + Sync + 'static {}
