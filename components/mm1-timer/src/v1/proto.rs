use mm1_core::message::AnyMessage;
use mm1_proto::message;
use tokio::time::Instant;

use crate::v1::OneshotKey;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct ScheduleOneshotAt {
    #[serde(with = "mm1_common::serde::no_serde")]
    pub at:      Instant,
    #[serde(with = "mm1_common::serde::no_serde")]
    pub message: AnyMessage,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct ScheduledOneshot {
    pub key: OneshotKey,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct CancelOneshot {
    pub key: OneshotKey,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct CanceledOneshot {
    #[serde(with = "mm1_common::serde::no_serde")]
    pub message: Option<AnyMessage>,
}
