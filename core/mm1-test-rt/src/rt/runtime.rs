use std::collections::hash_map::Entry::*;
use std::sync::Arc;

use futures::{FutureExt, StreamExt};
use mm1_address::address::Address;
use mm1_common::log;
use tokio::sync::{Mutex, mpsc};

use super::MainActorOutcome;
use crate::rt::actor_fn::ActorFn;
use crate::rt::{
    ActorEntry, ActorTaskOutcome, AddressLease, Context, Event, EventKind, ForkTaskOutcome,
    Runtime, RuntimeError, RuntimeShared, TaskKey,
};

impl<R> Runtime<R> {
    pub fn new() -> Self {
        let (queries_tx, queries_rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Mutex::new(RuntimeShared {
            queries_rx,
            tasks: Default::default(),
            entries: Default::default(),
        }));
        Self { queries_tx, shared }
    }

    pub fn new_context(
        &self,
        task_key: TaskKey,
        address_lease: Option<AddressLease>,
    ) -> Context<R> {
        let queries_tx = self.queries_tx.clone();
        Context {
            task_key,
            address_lease,
            queries_tx,
        }
    }

    pub async fn add_actor<A>(
        &self,
        actor_address: Address,
        address_lease: Option<AddressLease>,
        actor: A,
    ) -> Result<&Self, RuntimeError>
    where
        R: Send + 'static,
        A: for<'a> ActorFn<'a, R>,
    {
        let task_key = TaskKey::actor(actor_address);
        let context = self.new_context(task_key, None);
        let fut = async move {
            let mut context = context;
            let _ = actor.run(&mut context).await;
        };
        self.add_task(task_key, address_lease, fut).await?;
        Ok(self)
    }

    pub async fn with_actor<A>(
        self,
        actor_address: Address,
        address_lease: Option<AddressLease>,
        actor: A,
    ) -> Result<Self, RuntimeError>
    where
        R: Send + 'static,
        A: for<'a> ActorFn<'a, R>,
    {
        self.add_actor(actor_address, address_lease, actor).await?;
        Ok(self)
    }

    pub async fn add_task<F>(
        &self,
        task_key: TaskKey,
        address_lease: Option<AddressLease>,
        fut: F,
    ) -> Result<(), RuntimeError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.shared
            .lock()
            .await
            .add_task(task_key, address_lease, fut)
            .await
    }

    pub async fn next_event(&self) -> Result<Option<Event<R, EventKind<R>>>, RuntimeError> {
        let runtime = self.shared.clone();
        let mut shared = self.shared.lock().await;
        let RuntimeShared {
            queries_rx, tasks, ..
        } = &mut *shared;

        let next_query = queries_rx.recv();
        let task_done = tasks.next();

        let event_opt = tokio::select! {
            done = task_done => {
                if let Some(done) = done {
                    let event = Event {runtime, kind: EventKind::Done(done)};
                    Some(event)
                } else {
                    None
                }},
            query = next_query => {
                if let Some(query) = query {
                    let event = Event {runtime, kind: EventKind::Query(query)};
                    Some(event)
                } else {
                    None
                }},
        };

        Ok(event_opt)
    }
}

impl<R> Default for Runtime<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> RuntimeShared<R> {
    pub(crate) async fn add_task<F>(
        &mut self,
        task_key: TaskKey,
        address_lease: Option<AddressLease>,
        fut: F,
    ) -> Result<(), RuntimeError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        match (
            self.entries.entry(task_key.actor),
            task_key.actor == task_key.context,
        ) {
            (Vacant(_), false) => Err(RuntimeError::NoActor(task_key.actor)),
            (Vacant(vacant), true) => {
                log::trace!("adding main actor [key: {:?}]", task_key);

                let entry = ActorEntry {
                    forks:          Default::default(),
                    actor_canceled: Default::default(),
                    actor_done:     Default::default(),
                };
                let actor_canceled = entry.actor_canceled.clone();
                let actor_done = entry.actor_done.clone();
                let fut = async move {
                    tokio::select! {
                        _actor_canceled = actor_canceled.notified() => log::trace!("actor canceled [key: {:?}]", task_key),
                        _actor_completed = fut => log::trace!("actor completed [key: {:?}]", task_key),
                    }
                    actor_done.notify_waiters();
                    ActorTaskOutcome::Main(MainActorOutcome {
                        address: task_key.actor,
                        address_lease,
                    })
                }
                .boxed();

                vacant.insert(entry);
                self.tasks.push(fut);

                Ok(())
            },
            (Occupied(_), true) => Err(RuntimeError::Duplicate(task_key.actor)),
            (Occupied(occupied), false) => {
                log::trace!("adding fork task [key: {:?}]", task_key);
                let entry = occupied.into_mut();
                let true = entry.forks.insert(task_key.context) else {
                    return Err(RuntimeError::Duplicate(task_key.context))
                };
                let actor_done = entry.actor_done.clone();
                let fut = async move {
                    tokio::select! {
                        _fork_done = fut => log::trace!("fork task done [key: {:?}]", task_key),
                        _actor_done = actor_done.notified() => log::trace!("main actor notified completion [key: {:?}]", task_key),
                    };
                    ActorTaskOutcome::Fork(ForkTaskOutcome { task_key , address_lease, })
                }
                .boxed();
                self.tasks.push(fut);

                Ok(())
            },
        }
    }
}
