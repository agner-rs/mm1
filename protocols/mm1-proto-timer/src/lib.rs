// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use std::future::Future;
use std::hash::Hash;
use std::ops::{Add, Sub};

use mm1_proto::{Message, message};

pub trait Timer: Send + 'static {
    type Key: Ord + Clone + Send + Hash;
    type Instant: Copy
        + Send
        + Ord
        + Add<Self::Duration, Output = Self::Instant>
        + Sub<Self::Instant, Output = Self::Duration>;
    type Duration: Copy + Send + Default;
    type Message: Message;

    fn sleep(d: Self::Duration) -> impl Future<Output = ()> + Send;

    #[doc(hidden)]
    fn instant_plus_duration(t: Self::Instant, d: Self::Duration) -> Self::Instant {
        t + d
    }
}

#[cfg_attr(
    feature = "serde",
    derive(derive_more::Debug, serde::Serialize, serde::Deserialize)
)]
#[message]
pub struct ScheduleOnce<T: Timer> {
    pub key: T::Key,
    pub at:  T::Instant,
    pub msg: T::Message,
}

#[cfg_attr(
    feature = "serde",
    derive(derive_more::Debug, serde::Serialize, serde::Deserialize)
)]
#[message]
pub struct SchedulePeriodic<T: Timer>
where
    T::Message: Clone,
{
    pub key:    T::Key,
    pub delay:  T::Instant,
    pub period: T::Duration,
    pub msg:    T::Message,
}

#[cfg_attr(
    feature = "serde",
    derive(derive_more::Debug, serde::Serialize, serde::Deserialize)
)]
#[message]
pub struct Cancel<T: Timer> {
    pub key: T::Key,
}
