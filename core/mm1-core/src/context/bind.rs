use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum BindErrorKind {
    Closed,
    Conflict,
}

#[derive(Debug, Clone)]
pub struct BindArgs<Addr> {
    pub bind_to:    Addr,
    pub inbox_size: usize,
}

pub trait Bind<Addr> {
    fn bind(
        &mut self,
        args: BindArgs<Addr>,
    ) -> impl Future<Output = Result<(), ErrorOf<BindErrorKind>>> + Send;
}

impl_error_kind!(BindErrorKind);
