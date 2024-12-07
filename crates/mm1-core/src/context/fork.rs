use std::future::Future;

use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForkErrorKind {
    InternalError,
    ResourceConstraint,
}

pub trait Fork: Sized + Send + 'static {
    fn fork(&mut self) -> impl Future<Output = Result<Self, ErrorOf<ForkErrorKind>>> + Send;

    fn run<F, Fut>(self, fun: F) -> impl Future<Output = ()> + Send
    where
        F: FnOnce(Self) -> Fut,
        F: Send + 'static,
        Fut: std::future::Future + Send + 'static;
}

impl_error_kind!(ForkErrorKind);
