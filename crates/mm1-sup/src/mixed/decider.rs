use mm1_proto::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message]
pub enum DeciderErrorKind {}

pub trait Decider {
    type Key;
    type Addr;
    type Error;

    fn add(&mut self, key: Self::Key) -> Result<(), Self::Error>;
    fn rm(&mut self, key: &Self::Key) -> Option<()>;

    fn started(&mut self, key: &Self::Key, addr: Self::Addr) -> Result<(), Self::Error>;
    fn exited(&mut self, addr: Self::Addr) -> Result<(), Self::Error>;

    fn next_action(&mut self) -> Result<Action<'_, Self::Key>, Self::Error>;
}

pub enum Action<'a, ID> {
    Nothing,
    Start(&'a ID),
    Stop(&'a ID),
    Escalate,
}
