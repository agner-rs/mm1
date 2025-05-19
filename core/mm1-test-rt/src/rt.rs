use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use mm1_address::address::Address;
use mm1_address::pool::Lease as AddressLease;
use tokio::sync::{Mutex, Notify, mpsc};

mod actor_fn;
mod context;
pub mod event;
pub mod query;
mod runtime;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("no actor: {}", _0)]
    NoActor(Address),
    #[error("duplicate: {}", _0)]
    Duplicate(Address),
}

#[derive(Debug, Clone)]
pub struct Runtime<R> {
    queries_tx: mpsc::UnboundedSender<Query<R>>,
    shared:     Arc<Mutex<RuntimeShared<R>>>,
}

#[derive(derive_more::Debug)]
pub struct Event<R, K> {
    #[debug(skip)]
    runtime: Arc<Mutex<RuntimeShared<R>>>,

    pub kind: K,
}

#[derive(Debug)]
pub enum EventKind<R> {
    Done(ActorTaskOutcome),
    Query(Query<R>),
}

#[derive(derive_more::Debug)]
pub struct Context<R> {
    task_key:      TaskKey,
    queries_tx:    mpsc::UnboundedSender<Query<R>>,
    address_lease: Option<AddressLease>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskKey {
    pub actor:   Address,
    pub context: Address,
}

#[derive(Debug, derive_more::From)]
pub enum Query<R> {
    InitDone(query::InitDone),
    Spawn(query::Spawn<R>),
    Start(query::Start<R>),
    Recv(query::Recv),
    RecvClose(query::RecvClose),
    Fork(query::Fork<R>),
    ForkRun(query::ForkRun),
    Quit(query::Quit),
    Tell(query::Tell),
    Watch(query::Watch),
    Unwatch(query::Unwatch),
    Link(query::Link),
    Unlink(query::Unlink),
    SetTrapExit(query::SetTrapExit),
    Exit(query::Exit),
    Kill(query::Kill),
}

#[derive(derive_more::Debug)]
struct RuntimeShared<R> {
    queries_rx: mpsc::UnboundedReceiver<Query<R>>,
    tasks:      FuturesUnordered<BoxFuture<'static, ActorTaskOutcome>>,
    entries:    HashMap<Address, ActorEntry>,
}

#[derive(derive_more::Debug)]
struct ActorEntry {
    forks: HashSet<Address>,

    #[debug(skip)]
    actor_canceled: Arc<Notify>,

    #[debug(skip)]
    actor_done: Arc<Notify>,
}

#[derive(Debug)]
pub enum ActorTaskOutcome {
    Main(MainActorOutcome),
    Fork(ForkTaskOutcome),
}

#[derive(Debug)]
pub struct MainActorOutcome {
    pub address:       Address,
    pub address_lease: Option<AddressLease>,
}

#[derive(Debug)]
pub struct ForkTaskOutcome {
    pub task_key:      TaskKey,
    pub address_lease: Option<AddressLease>,
}
