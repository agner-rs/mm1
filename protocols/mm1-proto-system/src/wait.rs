use std::fmt;

use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub struct WatchRef(u64);

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Watch {
    pub peer: Address,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Unwatch {
    pub watch_ref: WatchRef,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct Down {
    pub peer:        Address,
    pub watch_ref:   WatchRef,
    pub normal_exit: bool,
}

impl WatchRef {
    pub const MAX: Self = WatchRef::from_u64(u64::MAX);
    pub const MIN: Self = WatchRef::from_u64(u64::MIN);

    pub const fn from_u64(v: u64) -> Self {
        Self(v)
    }

    pub const fn into_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WatchRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
