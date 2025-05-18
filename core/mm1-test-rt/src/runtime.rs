use std::collections::HashMap;
use std::fmt;
use std::pin::pin;

use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use mm1_address::address::Address;
use tokio::sync::mpsc;

use crate::context::TestContext;
use crate::query::Query;

#[derive(Debug, thiserror::Error)]
pub enum TestRuntimeError {
    #[error("no actor entry: {}", _0)]
    NoActorEntry(Address),
    #[error("duplicate address: {}", _0)]
    DuplicateAddress(Address),
}

#[derive(Debug)]
pub enum Event<R> {
    Query { address: Address, query: Query<R> },
    Done { address: Address },
}

impl<R> Event<R>
where
    Self: fmt::Debug,
{
    pub fn expect_done(self) -> Address {
        let Self::Done { address } = self else {
            panic!("nope: {:?}", self)
        };
        address
    }

    pub fn expect_query<Q>(self) -> (Address, Q)
    where
        Query<R>: TryInto<Q>,
        <Query<R> as TryInto<Q>>::Error: fmt::Debug,
    {
        let Self::Query { address, query } = self else {
            panic!("nope: {:?}", self)
        };
        (address, query.try_into().expect("nope"))
    }
}

#[derive(Debug)]
pub struct TestRuntime<R> {
    running:    FuturesUnordered<BoxFuture<'static, TestContext<R>>>,
    queries_tx: mpsc::UnboundedSender<(Address, Query<R>)>,
    queries_rx: mpsc::UnboundedReceiver<(Address, Query<R>)>,
    entries:    HashMap<Address, ActorEntry>,
}

#[derive(Debug)]
struct ActorEntry {
    command_tx: mpsc::UnboundedSender<Command>,
}

enum Command {
    Stop,
    AddFork(BoxFuture<'static, ()>),
}

impl<R> TestRuntime<R> {
    pub fn new() -> Self {
        let (queries_tx, queries_rx) = mpsc::unbounded_channel();
        Self {
            running: Default::default(),
            queries_rx,
            queries_tx,
            entries: Default::default(),
        }
    }

    pub fn with_actor<A>(
        mut self,
        actor_address: Address,
        actor: A,
    ) -> Result<Self, TestRuntimeError>
    where
        R: Send + 'static,
        A: for<'a> ActorFn<'a, R>,
    {
        self.add_actor(actor_address, actor)?;
        Ok(self)
    }

    pub fn add_actor<A>(
        &mut self,
        actor_address: Address,
        actor: A,
    ) -> Result<&mut Self, TestRuntimeError>
    where
        R: Send + 'static,
        A: for<'a> ActorFn<'a, R>,
    {
        use std::collections::hash_map::Entry::Vacant;

        let Vacant(vacant_entry) = self.entries.entry(actor_address) else {
            return Err(TestRuntimeError::DuplicateAddress(actor_address))
        };
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let context = TestContext {
            actor_address,
            context_address: actor_address,
            tx: self.queries_tx.clone(),
            inbox_closed: false,
        };
        let actor_running = run_actor(command_rx, context, actor);
        let actor_entry = ActorEntry { command_tx };
        self.running.push(actor_running.boxed());
        vacant_entry.insert(actor_entry);

        Ok(self)
    }

    pub fn new_fork_context(
        &mut self,
        actor_address: Address,
        fork_address: Address,
    ) -> TestContext<R> {
        TestContext {
            actor_address,
            context_address: fork_address,
            tx: self.queries_tx.clone(),
            inbox_closed: false,
        }
    }

    pub async fn next_event(&mut self) -> Option<Event<R>> {
        let next_done = async {
            let context = self.running.next().await?;
            Some(Event::<R>::Done {
                address: context.actor_address,
            })
        };
        let next_query = async {
            let (address, query) = self.queries_rx.recv().await?;
            Some(Event::Query { address, query })
        };

        tokio::select! {
            done = next_done => done,
            query = next_query => query,
        }
    }

    pub fn rm_actor(&mut self, actor_address: Address) -> bool {
        let Some(actor_entry) = self.entries.remove(&actor_address) else {
            return false
        };
        let _ = actor_entry.command_tx.send(Command::Stop);
        true
    }

    pub fn add_fork_task(
        &mut self,
        actor_address: Address,
        fork_task: BoxFuture<'static, ()>,
    ) -> Result<(), TestRuntimeError> {
        let Some(actor_entry) = self.entries.get_mut(&actor_address) else {
            return Err(TestRuntimeError::NoActorEntry(actor_address))
        };
        let _ = actor_entry.command_tx.send(Command::AddFork(fork_task));
        Ok(())
    }
}

async fn run_actor<R, A>(
    mut command_rx: mpsc::UnboundedReceiver<Command>,
    mut context: TestContext<R>,
    actor: A,
) -> TestContext<R>
where
    A: for<'a> ActorFn<'a, R>,
{
    {
        let mut forks = FuturesUnordered::<BoxFuture<'static, ()>>::new();
        forks.push(std::future::pending().boxed());

        let mut actor_running = pin!(actor.run(&mut context).fuse());

        loop {
            let next_fork_done = forks.next().fuse();
            tokio::select! {
                command_opt = command_rx.recv() =>
                    if let Some(command) = command_opt {
                        match command {
                            Command::Stop => break,
                            Command::AddFork(f) => forks.push(f),
                        }
                    } else {
                        break
                    },
                _ = next_fork_done => (),
                _ = actor_running.as_mut() => break,
            }
        }
    }

    context
}

impl<R> Default for TestRuntime<R> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ActorFn<'a, R>: Send + 'a {
    type Fut: Future + Send + 'a;
    type Out;

    fn run(self, context: &'a mut TestContext<R>) -> Self::Fut;
}
impl<'a, R, Fun, Fut> ActorFn<'a, R> for Fun
where
    R: 'a,
    Self: Send + 'a,
    Fun: FnOnce(&'a mut TestContext<R>) -> Fut,
    Fut: Future + Send + 'a,
{
    type Fut = Fut;
    type Out = Fut::Output;

    fn run(self, context: &'a mut TestContext<R>) -> Self::Fut {
        (self)(context)
    }
}

macro_rules! impl_actor_func_with_args {
    ( $( $t:ident ),* $(,)? ) => {
        impl<'a, R, Fun, Fut,
                $( $t , )*
            > ActorFn<'a, R> for (Fun, (
                $( $t , )*
            ))
        where
            R: Send + 'a,
            Self: Send + 'a,
            Fun: FnOnce(
                    &'a mut TestContext<R>,
                    $( $t , )*
                ) -> Fut,
            Fut: Future + Send + 'a,
        {
            type Fut = Fut;
            type Out = Fut::Output;

            fn run(self, context: &'a mut TestContext<R>) -> Self::Fut {
                #[allow(non_snake_case)]
                let (f, (
                        $( $t , )*
                    )) = self;
                (f)(
                    context,
                    $( $t , )*
                )
            }
        }
    };
}

impl_actor_func_with_args!(T0);
impl_actor_func_with_args!(T0, T1);
impl_actor_func_with_args!(T0, T1, T2);
impl_actor_func_with_args!(T0, T1, T2, T3);
impl_actor_func_with_args!(T0, T1, T2, T3, T4);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13);
impl_actor_func_with_args!(T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
impl_actor_func_with_args!(
    T0, T1, T2, T3, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
