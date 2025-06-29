use std::fmt;

use mm1_address::address::Address;
use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum DeciderErrorKind {}

pub trait Decider {
    type Key;
    type Error: fmt::Display;

    fn add(&mut self, key: Self::Key) -> Result<(), Self::Error>;
    fn rm(&mut self, key: &Self::Key) -> Result<(), Self::Error>;

    fn started(&mut self, key: &Self::Key, addr: Address, at: tokio::time::Instant);
    fn exited(&mut self, addr: Address, normal_exit: bool, at: tokio::time::Instant);
    fn failed(&mut self, key: &Self::Key, at: tokio::time::Instant);
    fn quit(&mut self, normal_exit: bool);

    fn next_action(
        &mut self,
        at: tokio::time::Instant,
    ) -> Result<Option<Action<'_, Self::Key>>, Self::Error>;
}

#[derive(Debug)]
pub enum Action<'a, ID> {
    Noop,
    InitDone,
    Start {
        child_id: &'a ID,
    },
    Stop {
        address:  Address,
        child_id: Option<&'a ID>,
    },
    Quit {
        normal_exit: bool,
    },
}

impl<ID> fmt::Display for Action<'_, ID>
where
    ID: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Noop => write!(f, "Noop"),
            Self::InitDone => write!(f, "InitDone"),
            Self::Start { child_id } => write!(f, "Start({child_id})"),
            Self::Stop { address, child_id } => {
                write!(
                    f,
                    "Stop({}, {:?})",
                    address,
                    child_id.map(|s| s.to_string())
                )
            },
            Self::Quit { normal_exit } => write!(f, "Quit(normal={normal_exit})"),
        }
    }
}
