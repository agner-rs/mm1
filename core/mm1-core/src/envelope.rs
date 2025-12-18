use std::any::TypeId;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use mm1_address::address::Address;
pub use mm1_proc_macros::dispatch;
use mm1_proto::Message;

use crate::message::AnyMessage;
use crate::tracing::TraceId;

static ENVELOPE_SEQ_NO: AtomicU64 = AtomicU64::new(0);

const DEFAULT_TTL: u8 = 15;

#[derive(Debug)]
pub struct EnvelopeHeader {
    pub to:       Address,
    pub ttl:      u8,
    pub priority: bool,
    pub no:       u64,
    pub trace_id: TraceId,
}

pub struct Envelope<M = AnyMessage> {
    header:  EnvelopeHeader,
    message: M,
}

impl EnvelopeHeader {
    pub fn to_address(to: Address) -> Self {
        Self {
            to,
            no: ENVELOPE_SEQ_NO.fetch_add(1, Ordering::Relaxed),
            priority: false,
            ttl: DEFAULT_TTL,
            trace_id: TraceId::current(),
        }
    }

    pub fn with_ttl(self, ttl: u8) -> Self {
        Self { ttl, ..self }
    }

    pub fn with_trace_id(self, trace_id: TraceId) -> Self {
        Self { trace_id, ..self }
    }

    pub fn with_priority(self, priority: bool) -> Self {
        Self { priority, ..self }
    }

    pub fn with_no(self, no: u64) -> Self {
        Self { no, ..self }
    }

    pub fn trace_id(&self) -> TraceId {
        self.trace_id
    }
}

impl<M> Envelope<M>
where
    M: Message,
{
    pub fn into_erased(self) -> Envelope<AnyMessage> {
        let Self {
            header: info,
            message,
        } = self;
        let message = AnyMessage::new(message);
        Envelope {
            header: info,
            message,
        }
    }
}

impl<M> Envelope<M> {
    pub fn new(header: EnvelopeHeader, message: M) -> Self {
        Self { header, message }
    }

    pub fn header(&self) -> &EnvelopeHeader {
        &self.header
    }
}

impl Envelope<AnyMessage> {
    pub fn cast<M>(self) -> Result<Envelope<M>, Self>
    where
        M: Message,
    {
        let Self {
            header: info,
            message,
        } = self;
        match message.cast() {
            Ok(message) => {
                Ok(Envelope {
                    header: info,
                    message,
                })
            },
            Err(message) => {
                Err(Self {
                    header: info,
                    message,
                })
            },
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
        self.message.is::<M>()
    }

    pub fn tid(&self) -> TypeId {
        self.message.tid()
    }

    pub fn message_name(&self) -> &str {
        self.message.type_name()
    }
}
impl<M> Envelope<M> {
    pub fn take(self) -> (M, Envelope<()>) {
        (
            self.message,
            Envelope {
                header:  self.header,
                message: (),
            },
        )
    }
}

impl From<Envelope<()>> for EnvelopeHeader {
    fn from(value: Envelope<()>) -> Self {
        let Envelope {
            header,
            message: (),
        } = value;
        header
    }
}

impl<M> fmt::Debug for Envelope<M>
where
    M: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope")
            .field("info", &self.header)
            .field("message", &self.message)
            .finish()
    }
}
