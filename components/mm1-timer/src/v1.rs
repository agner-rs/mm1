use std::collections::BTreeSet;
use std::hash::Hash;
use std::time::Duration;

use futures::future;
use mm1_address::address::Address;
use mm1_ask::{Ask, AskErrorKind, Reply};
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::types::AnyError;
use mm1_core::context::{Fork, ForkErrorKind, Messaging, Now, RecvErrorKind};
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1_core::message::AnyMessage;
use mm1_core::tracing::TraceId;
use mm1_proto_ask::Request;
use slotmap::SlotMap;
use tokio::time::Instant;

#[doc(hidden)]
pub mod proto;

const ASK_TIMEOUT: Duration = Duration::from_millis(100);

slotmap::new_key_type! {
    pub struct OneshotKey;
}

#[derive(Debug, thiserror::Error)]
pub enum TimerError {
    #[error("{}", _0)]
    Fork(
        #[source]
        #[from]
        ErrorOf<ForkErrorKind>,
    ),

    #[error("{}", _0)]
    Ask(
        #[source]
        #[from]
        ErrorOf<AskErrorKind>,
    ),
}

#[derive(Debug, Clone)]
pub struct OneshotTimer<Ctx> {
    client_ctx:  Ctx,
    server_addr: Address,
}

impl<Ctx> OneshotTimer<Ctx>
where
    Ctx: Ask + Fork + Messaging + Now<Instant = Instant>,
{
    pub async fn create(context: &mut Ctx) -> Result<Self, TimerError> {
        let client_ctx = context.fork().await?;
        let server_ctx = context.fork().await?;

        let receiver_addr = context.address();
        let server_addr = server_ctx.address();
        server_ctx
            .run(move |server_ctx| timer_run(server_ctx, receiver_addr))
            .await;

        Ok(Self {
            client_ctx,
            server_addr,
        })
    }

    pub async fn schedule_once_at<M: Into<AnyMessage>>(
        &mut self,
        at: Instant,
        message: M,
    ) -> Result<OneshotKey, TimerError> {
        let message = message.into();
        let proto::ScheduledOneshot { key } = self
            .client_ctx
            .ask(
                self.server_addr,
                proto::ScheduleOneshotAt { at, message },
                ASK_TIMEOUT,
            )
            .await?;
        Ok(key)
    }

    pub async fn schedule_once_after<M: Into<AnyMessage>>(
        &mut self,
        after: Duration,
        message: M,
    ) -> Result<OneshotKey, TimerError> {
        let at = self.client_ctx.now() + after;
        self.schedule_once_at(at, message).await
    }

    pub async fn cancel(&mut self, key: OneshotKey) -> Result<Option<AnyMessage>, TimerError> {
        let proto::CanceledOneshot { message } = self
            .client_ctx
            .ask(self.server_addr, proto::CancelOneshot { key }, ASK_TIMEOUT)
            .await?;
        Ok(message)
    }
}

#[derive(Default)]
struct TimerState {
    #[allow(dead_code)]
    entries: SlotMap<OneshotKey, Entry>,
    ordered: BTreeSet<(Instant, OneshotKey)>,
}

#[allow(dead_code)]
struct Entry {
    at:      Instant,
    message: AnyMessage,
}

async fn timer_run<Ctx>(mut ctx: Ctx, receiver_addr: Address) -> Result<(), AnyError>
where
    Ctx: Messaging + Reply,
{
    enum Event {
        Time,
        Schedule(Request<proto::ScheduleOneshotAt>),
        Cancel(Request<proto::CancelOneshot>),
        RecvError(ErrorOf<RecvErrorKind>),
    }

    let mut state: TimerState = Default::default();

    loop {
        let time = async {
            if let Some((at, _)) = state.ordered.first().copied() {
                tokio::time::sleep_until(at).await;
                Event::Time
            } else {
                future::pending().await
            }
        };
        let received = async {
            match ctx.recv().await {
                Ok(envelope) => {
                    dispatch!(match envelope {
                        msg @ Request::<proto::ScheduleOneshotAt> { .. } => Event::Schedule(msg),
                        msg @ Request::<proto::CancelOneshot> { .. } => Event::Cancel(msg),
                    })
                },
                Err(reason) => Event::RecvError(reason),
            }
        };

        let event = tokio::select! {
            e = time => e,
            e = received => e,
        };

        match event {
            Event::RecvError(reason) => return Err(reason.into()),
            Event::Schedule(Request {
                header,
                payload: proto::ScheduleOneshotAt { at, message },
            }) => {
                let key = state.entries.insert(Entry { at, message });
                state.ordered.insert((at, key));
                ctx.reply(header, proto::ScheduledOneshot { key }).await?;
            },
            Event::Cancel(Request {
                header,
                payload: proto::CancelOneshot { key },
            }) => {
                let reply = if let Some(Entry { at, message }) = state.entries.remove(key) {
                    let _ = state.ordered.remove(&(at, key));
                    proto::CanceledOneshot {
                        message: Some(message),
                    }
                } else {
                    proto::CanceledOneshot { message: None }
                };
                ctx.reply(header, reply).await?;
            },
            Event::Time => {
                let now = Instant::now();

                loop {
                    let Some((at, _)) = state.ordered.first().copied() else {
                        break
                    };
                    if at > now {
                        break
                    }
                    let (_, key) = state.ordered.pop_first().expect("just checked");
                    let Entry { at: _, message } = state.entries.remove(key).expect("should exist");
                    let header =
                        EnvelopeHeader::to_address(receiver_addr).with_trace_id(TraceId::random());
                    let envelope = Envelope::new(header, message);
                    ctx.send(envelope).await?;
                }
            },
        }
    }
}
