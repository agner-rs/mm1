use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
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
        Fut: Future + Send + 'static;
}

impl_error_kind!(ForkErrorKind);
