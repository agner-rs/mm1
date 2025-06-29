use core::fmt;

use mm1_address::address::Address;
use mm1_address::pool::Lease;
use mm1_proto_system::WatchRef;

#[derive(Debug)]
pub(crate) enum SysMsg {
    Kill,
    Link(SysLink),
    Watch(SysWatch),
    ForkDone(Lease),
}

#[derive(Debug)]
pub(crate) enum SysLink {
    Connect {
        sender:   Address,
        receiver: Address,
    },
    Disconnect {
        sender:   Address,
        receiver: Address,
    },
    Exit {
        sender:   Address,
        receiver: Address,
        reason:   ExitReason,
    },
}

#[derive(Debug)]
pub(crate) enum SysWatch {
    Watch {
        sender:    Address,
        receiver:  Address,
        watch_ref: WatchRef,
    },
    Unwatch {
        sender:    Address,
        receiver:  Address,
        watch_ref: WatchRef,
    },
    Down {
        sender:   Address,
        receiver: Address,
        reason:   ExitReason,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExitReason {
    Normal,
    Terminate,
    LinkDown,
}

impl fmt::Display for SysLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { sender, receiver } => {
                write!(f, "connect [sender: {}; receiver: {}]", sender, receiver)
            },
            Self::Disconnect { sender, receiver } => {
                write!(f, "disconnect [sender: {}; receiver: {}]", sender, receiver)
            },
            Self::Exit {
                sender,
                receiver,
                reason,
            } => {
                write!(
                    f,
                    "exit [sender: {}; receiver: {}; reason: {:?}]",
                    sender, receiver, reason,
                )
            },
        }
    }
}

impl fmt::Display for SysWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Watch {
                sender,
                receiver,
                watch_ref,
            } => {
                write!(
                    f,
                    "watch[sender: {}; receiver: {}; ref: {}]",
                    sender, receiver, watch_ref
                )
            },
            Self::Unwatch {
                sender,
                receiver,
                watch_ref,
            } => {
                write!(
                    f,
                    "unwatch[sender: {}; receiver: {}; ref: {}]",
                    sender, receiver, watch_ref
                )
            },
            Self::Down {
                sender,
                receiver,
                reason,
            } => {
                write!(
                    f,
                    "down[sender: {}; receiver: {}; reason: {:?}]",
                    sender, receiver, reason
                )
            },
        }
    }
}

impl fmt::Display for SysMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForkDone(address_lease) => {
                write!(f, "fork-done[address: {}]", address_lease.net_address())
            },
            Self::Kill => write!(f, "kill"),
            Self::Link(l) => write!(f, "link/{}", l),
            Self::Watch(w) => write!(f, "watch/{}", w),
        }
    }
}
