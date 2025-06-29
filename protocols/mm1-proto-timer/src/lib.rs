// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use std::hash::Hash;
use std::ops::{Add, Sub};

use mm1_proto::{Message, message};

pub trait Timer: Send + 'static {
    type Key: Message + Ord + Clone + Send + Hash;
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

#[message(base_path = ::mm1_proto)]
pub struct ScheduleOnce<T: Timer> {
    pub key: T::Key,
    #[serde(skip)]
    pub at:  T::Instant,
    pub msg: T::Message,
}

#[message(base_path = ::mm1_proto)]
pub struct SchedulePeriodic<T: Timer>
where
    T::Message: Clone,
{
    pub key:    T::Key,
    pub delay:  T::Instant,
    pub period: T::Duration,
    pub msg:    T::Message,
}

#[message(base_path = ::mm1_proto)]
pub struct Cancel<T: Timer> {
    pub key: T::Key,
}
