use std::time::Duration;

use futures::future::BoxFuture;
use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::types::Never;
use mm1_core::context::{ForkErrorKind, RecvErrorKind, TellErrorKind};
use mm1_core::envelope::Envelope;
use mm1_proto_system::{SpawnErrorKind, StartErrorKind, WatchRef};
use tokio::sync::oneshot;

use crate::context::TestContext;

#[derive(Debug, derive_more::From, derive_more::TryInto)]
pub enum Query<R> {
    InitDone(InitDone),
    Recv(Recv),
    Tell(Tell),
    Fork(Fork<R>),
    Run(Run),
    Link(Link),
    Unlink(Unlink),
    SetTrapExit(SetTrapExit),
    QuitOk(QuitOk),
    QuitErr(QuitErr),
    Exit(Exit),
    Kill(Kill),
    Spawn(Spawn<R>),
    Start(Start<R>),
    Watch(Watch),
    Unwatch(Unwatch),
}

#[derive(derive_more::Debug)]
pub struct InitDone {
    pub address:    Address,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Recv {
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<Result<Envelope, ErrorOf<RecvErrorKind>>>,
}
#[derive(derive_more::Debug)]
pub struct Tell {
    pub envelope:   Envelope,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<Result<(), ErrorOf<TellErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Fork<R> {
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<Result<TestContext<R>, ErrorOf<ForkErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Run {
    #[debug(skip)]
    pub task:       BoxFuture<'static, ()>,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Link {
    pub this:       Address,
    pub peer:       Address,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Unlink {
    pub this:       Address,
    pub peer:       Address,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct SetTrapExit {
    pub enable:     bool,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct QuitOk {
    #[debug(skip)]
    pub(crate) _outcome_tx: oneshot::Sender<Never>,
}

#[derive(derive_more::Debug)]
pub struct QuitErr {
    pub reason:             Box<dyn std::error::Error + Send + Sync>,
    #[debug(skip)]
    pub(crate) _outcome_tx: oneshot::Sender<Never>,
}

#[derive(derive_more::Debug)]
pub struct Exit {
    pub peer:       Address,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<bool>,
}

#[derive(derive_more::Debug)]
pub struct Kill {
    pub peer:       Address,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<bool>,
}

#[derive(derive_more::Debug)]
pub struct Spawn<R> {
    pub runnable:   R,
    pub link:       bool,
    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<Result<Address, ErrorOf<SpawnErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Start<R> {
    pub runnable:      R,
    pub link:          bool,
    pub start_timeout: Duration,
    #[debug(skip)]
    pub outcome_tx:    oneshot::Sender<Result<Address, ErrorOf<StartErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Watch {
    pub this: Address,
    pub peer: Address,

    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<WatchRef>,
}

#[derive(derive_more::Debug)]
pub struct Unwatch {
    pub this:      Address,
    pub watch_ref: WatchRef,

    #[debug(skip)]
    pub outcome_tx: oneshot::Sender<()>,
}
