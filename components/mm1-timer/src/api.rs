use mm1_common::errors::error_of::ErrorOf;
use mm1_core::context::{ForkErrorKind, SendErrorKind};
use mm1_proto_timer as t;

pub trait TimerApi: Send + 'static {
    type Timer: t::Timer<
            Key = Self::Key,
            Instant = Self::Instant,
            Duration = Self::Duration,
            Message = Self::Message,
        >;

    type Instant;
    type Duration;
    type Key;
    type Message;

    fn cancel(&mut self, key: Self::Key) -> impl Future<Output = Result<(), TimerError>> + Send;

    fn schedule_once_at(
        &mut self,
        key: Self::Key,
        at: Self::Instant,
        msg: Self::Message,
    ) -> impl Future<Output = Result<(), TimerError>> + Send;

    fn schedule_once_after(
        &mut self,
        key: Self::Key,
        after: Self::Duration,
        msg: Self::Message,
    ) -> impl Future<Output = Result<(), TimerError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum TimerError {
    #[error("fork: {}", _0)]
    Fork(ErrorOf<ForkErrorKind>),
    #[error("send: {}", _0)]
    Send(ErrorOf<SendErrorKind>),
}
