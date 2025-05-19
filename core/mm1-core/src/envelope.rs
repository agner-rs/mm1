use std::any::TypeId;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use mm1_address::address::Address;
pub use mm1_proc_macros::dispatch;
use mm1_proto::Message;

use crate::message::AnyMessage;

static ENVELOPE_SEQ_NO: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct EnvelopeInfo {
    pub to:         Address,
    #[allow(dead_code)]
    no:             u64,
    #[allow(dead_code)]
    trace_id:       Option<u64>,
    #[allow(dead_code)]
    correlation_id: Option<u64>,
}

pub struct Envelope<M = AnyMessage> {
    info:    EnvelopeInfo,
    message: M,
}

impl EnvelopeInfo {
    pub fn new(to: Address) -> Self {
        Self {
            to,
            no: ENVELOPE_SEQ_NO.fetch_add(1, Ordering::Relaxed),
            trace_id: None,
            correlation_id: None,
        }
    }
}

impl<M> Envelope<M>
where
    M: Message,
{
    pub fn into_erased(self) -> Envelope<AnyMessage> {
        let Self { info, message } = self;
        let message = AnyMessage::new(message);
        Envelope { info, message }
    }
}

impl<M> Envelope<M> {
    pub fn new(info: EnvelopeInfo, message: M) -> Self {
        Self { info, message }
    }

    pub fn info(&self) -> &EnvelopeInfo {
        &self.info
    }
}

impl Envelope<AnyMessage> {
    pub fn cast<M>(self) -> Result<Envelope<M>, Self>
    where
        M: Message,
    {
        let Self { info, message } = self;
        match message.cast() {
            Ok(message) => Ok(Envelope { info, message }),
            Err(message) => Err(Self { info, message }),
        }
    }

    pub fn peek<M>(&self) -> Option<&M>
    where
        M: Message,
    {
        self.message.peek()
    }

    pub fn is<M>(&self) -> bool
    where
        M: Message,
    {
        // XXX that is a bit sloppy, alright
        self.message.peek::<M>().is_some()
    }

    pub fn tid(&self) -> TypeId {
        self.message.tid()
    }

    pub fn message_name(&self) -> &'static str {
        self.message.type_name()
    }
}
impl<M> Envelope<M> {
    pub fn take(self) -> (M, Envelope<()>) {
        (
            self.message,
            Envelope {
                info:    self.info,
                message: (),
            },
        )
    }
}

impl<M> fmt::Debug for Envelope<M>
where
    M: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope")
            .field("info", &self.info)
            .field("message", &self.message)
            .finish()
    }
}
