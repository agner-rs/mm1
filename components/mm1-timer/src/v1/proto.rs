use mm1_core::message::AnyMessage;
use mm1_proto::message;
use tokio::time::Instant;

use crate::v1::OneshotKey;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct ScheduleOneshotAt {
    #[serde(with = "no_serde")]
    pub at:      Instant,
    #[serde(with = "no_serde")]
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
    #[serde(with = "no_serde")]
    pub message: Option<AnyMessage>,
}

mod no_serde {
    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S, T>(_: T, _: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reason = S::Error::custom("not supported");
        Err(reason)
    }
    pub(super) fn deserialize<'de, D, T>(_: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
    {
        let reason = D::Error::custom("not supported");
        Err(reason)
    }
}
