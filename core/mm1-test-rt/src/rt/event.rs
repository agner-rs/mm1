use std::fmt;
use std::ops::Deref;

use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_core::context::{BindErrorKind, ForkErrorKind, RecvErrorKind, SendErrorKind};
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_proto_system::{SpawnErrorKind, StartErrorKind, WatchRef};
use tokio::sync::oneshot;

use super::{RuntimeError, TestContext};
use crate::rt::{
    ActorTaskOutcome, Event, EventKind, ForkTaskOutcome, MainActorOutcome, Query, query,
};

pub trait EventResolve<Outcome> {
    fn resolve(self, outcome: Outcome);
}

pub trait EventResolveResult<T, E> {
    fn resolve_ok(self, outcome_ok: T);
    fn resolve_err(self, outcome_err: E);
}

pub trait EventResolveErrorOfKind<K> {
    fn resolve_err_kind(self, outcome_err_kind: K);
}

impl<R, E> Deref for Event<R, E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.kind
    }
}

impl<R, K, O> EventResolve<O> for Event<R, K>
where
    K: Into<oneshot::Sender<O>>,
{
    fn resolve(self, outcome: O) {
        let Self { kind, .. } = self;
        let sender: oneshot::Sender<O> = kind.into();
        let _ = sender.send(outcome);
    }
}
impl<T, O, E> EventResolveResult<O, E> for T
where
    T: EventResolve<Result<O, E>>,
{
    fn resolve_ok(self, outcome_ok: O) {
        self.resolve(Ok(outcome_ok));
    }

    fn resolve_err(self, outcome_err: E) {
        self.resolve(Err(outcome_err));
    }
}

impl<R, K> Event<R, K> {
    pub fn convert<K1>(self) -> Result<Event<R, K1>, Self>
    where
        Event<R, K1>: TryFrom<Event<R, K>, Error = Self>,
    {
        self.try_into()
    }

    pub fn expect<K1>(self) -> Event<R, K1>
    where
        Event<R, K1>: TryFrom<Event<R, K>, Error = Self>,
        K: fmt::Debug,
    {
        self.convert::<K1>()
            .map_err(|actual| {
                format!(
                    "expected: {}; actual: {:?}",
                    std::any::type_name::<K1>(),
                    actual,
                )
            })
            .unwrap()
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, ActorTaskOutcome> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Done(kind) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, MainActorOutcome> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Done(ActorTaskOutcome::Main(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, ForkTaskOutcome> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Done(ActorTaskOutcome::Fork(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, Query<R>> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(kind) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Spawn<R>> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Spawn(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}
impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Start<R>> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Start(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Bind<NetAddress>> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::BindNetAddress(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Recv> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Recv(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::RecvClose> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::RecvClose(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Fork<R>> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Fork(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::ForkRun> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::ForkRun(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Quit> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Quit(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Tell> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Tell(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Watch> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Watch(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Unwatch> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Unwatch(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Link> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Link(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Unlink> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Unlink(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::SetTrapExit> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::SetTrapExit(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::InitDone> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::InitDone(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Exit> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Exit(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> TryFrom<Event<R, EventKind<R>>> for Event<R, query::Kill> {
    type Error = Event<R, EventKind<R>>;

    fn try_from(value: Event<R, EventKind<R>>) -> Result<Self, Self::Error> {
        let Event { runtime, kind } = value;
        match kind {
            EventKind::Query(Query::Kill(kind)) => Ok(Event { runtime, kind }),
            kind => Err(Event { runtime, kind }),
        }
    }
}

impl<R> Event<R, query::Spawn<R>> {
    pub fn take_runnable(self) -> (R, Event<R, query::Spawn<()>>) {
        let Self { runtime, kind } = self;
        let query::Spawn {
            task_key,
            runnable,
            link,
            outcome_tx,
        } = kind;
        let kind = query::Spawn {
            task_key,
            runnable: (),
            link,
            outcome_tx,
        };
        let event = Event { runtime, kind };
        (runnable, event)
    }
}

impl<R> Event<R, query::Tell> {
    pub fn take_envelope(&mut self) -> Envelope {
        let info = EnvelopeHeader::to_address(self.kind.envelope.header().to);
        std::mem::replace(
            &mut self.kind.envelope,
            Envelope::new(info, ()).into_erased(),
        )
    }
}

impl<R> Event<R, query::Start<R>> {
    pub fn take_runnable(self) -> (R, Event<R, query::Start<()>>) {
        let Self { runtime, kind } = self;
        let query::Start {
            task_key,
            runnable,
            link,
            start_timeout,
            outcome_tx,
        } = kind;
        let kind = query::Start {
            task_key,
            runnable: (),
            link,
            start_timeout,
            outcome_tx,
        };
        let event = Event { runtime, kind };
        (runnable, event)
    }
}

impl<R> Event<R, query::ForkRun> {
    pub fn resolve(self) -> Event<R, query::PendingTask> {
        let Self { runtime, kind } = self;
        let query::ForkRun {
            task_key,
            address_lease,
            task_fut,
            outcome_tx,
        } = kind;
        let _ = outcome_tx.send(());

        let kind = query::PendingTask {
            task_key,
            address_lease,
            task_fut,
        };
        Event { runtime, kind }
    }
}

impl<R> Event<R, query::PendingTask> {
    pub async fn run(self) -> Result<(), RuntimeError> {
        let Self { runtime, kind } = self;
        let query::PendingTask {
            task_key,
            address_lease,
            task_fut,
        } = kind;
        let mut runtime = runtime.lock().await;
        runtime.add_task(task_key, address_lease, task_fut).await?;
        Ok(())
    }
}

impl<R> Event<R, query::Quit> {
    pub async fn stop_tasks(self) -> Result<(), RuntimeError> {
        let Self { runtime, kind } = self;
        let runtime = runtime.lock().await;
        let actor_entry = runtime
            .entries
            .get(&kind.task_key.actor)
            .ok_or(RuntimeError::NoActor(kind.task_key.actor))?;
        actor_entry.actor_canceled.notify_waiters();
        Ok(())
    }
}

impl<R> Event<R, MainActorOutcome> {
    pub async fn remove_actor_entry(self) -> Result<(), RuntimeError> {
        let Self { runtime, kind } = self;
        let _ = runtime
            .lock()
            .await
            .entries
            .remove(&kind.address)
            .ok_or(RuntimeError::NoActor(kind.address))?;
        Ok(())
    }
}

impl<R> From<query::Spawn<R>> for oneshot::Sender<Result<Address, ErrorOf<SpawnErrorKind>>> {
    fn from(value: query::Spawn<R>) -> Self {
        value.outcome_tx
    }
}

impl<R> From<query::Start<R>> for oneshot::Sender<Result<Address, ErrorOf<StartErrorKind>>> {
    fn from(value: query::Start<R>) -> Self {
        value.outcome_tx
    }
}

impl<A> From<query::Bind<A>> for oneshot::Sender<Result<(), ErrorOf<BindErrorKind>>> {
    fn from(value: query::Bind<A>) -> Self {
        value.outcome_tx
    }
}

impl From<query::Recv> for oneshot::Sender<Result<Envelope, ErrorOf<RecvErrorKind>>> {
    fn from(value: query::Recv) -> Self {
        value.outcome_tx
    }
}
impl From<query::RecvClose> for oneshot::Sender<()> {
    fn from(value: query::RecvClose) -> Self {
        value.outcome_tx
    }
}

impl<R> From<query::Fork<R>> for oneshot::Sender<Result<TestContext<R>, ErrorOf<ForkErrorKind>>> {
    fn from(value: query::Fork<R>) -> Self {
        value.outcome_tx
    }
}

impl From<query::Tell> for oneshot::Sender<Result<(), ErrorOf<SendErrorKind>>> {
    fn from(value: query::Tell) -> Self {
        value.outcome_tx
    }
}

impl From<query::Watch> for oneshot::Sender<WatchRef> {
    fn from(value: query::Watch) -> Self {
        value.outcome_tx
    }
}
impl From<query::Unwatch> for oneshot::Sender<()> {
    fn from(value: query::Unwatch) -> Self {
        value.outcome_tx
    }
}

impl From<query::Link> for oneshot::Sender<()> {
    fn from(value: query::Link) -> Self {
        value.outcome_tx
    }
}

impl From<query::Unlink> for oneshot::Sender<()> {
    fn from(value: query::Unlink) -> Self {
        value.outcome_tx
    }
}

impl From<query::SetTrapExit> for oneshot::Sender<()> {
    fn from(value: query::SetTrapExit) -> Self {
        value.outcome_tx
    }
}

impl From<query::InitDone> for oneshot::Sender<()> {
    fn from(value: query::InitDone) -> Self {
        value.outcome_tx
    }
}

impl From<query::Exit> for oneshot::Sender<bool> {
    fn from(value: query::Exit) -> Self {
        value.outcome_tx
    }
}

impl From<query::Kill> for oneshot::Sender<bool> {
    fn from(value: query::Kill) -> Self {
        value.outcome_tx
    }
}
