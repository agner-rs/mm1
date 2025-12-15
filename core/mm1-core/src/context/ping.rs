use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

pub trait Ping: Send {
    // fn set_trap_ping(&mut self, enable: bool) -> impl Future<Output = ()>;
    fn ping(
        &mut self,
        address: Address,
        timeout: Duration,
    ) -> impl Future<Output = Result<Duration, ErrorOf<PingErrorKind>>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum PingErrorKind {
    Timeout,
    Send,
    Recv,
}

impl_error_kind!(PingErrorKind);
