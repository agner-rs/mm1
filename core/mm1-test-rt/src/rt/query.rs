use std::time::Duration;

use futures::future::BoxFuture;
use mm1_address::address::Address;
use mm1_address::pool::Lease as AddressLease;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::types::Never;
use mm1_core::context::{ForkErrorKind, RecvErrorKind, TellErrorKind};
use mm1_core::envelope::Envelope;
use mm1_proto_system::{SpawnErrorKind, StartErrorKind, WatchRef};
use tokio::sync::oneshot;

use super::Context;
use crate::rt::TaskKey;

#[derive(derive_more::Debug)]
pub struct Spawn<R> {
    pub task_key: TaskKey,
    pub link:     bool,

    pub(crate) runnable: R,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<Result<Address, ErrorOf<SpawnErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Start<R> {
    pub task_key:      TaskKey,
    pub link:          bool,
    pub start_timeout: Duration,

    pub(crate) runnable: R,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<Result<Address, ErrorOf<StartErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct InitDone {
    pub task_key: TaskKey,
    pub address:  Address,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Recv {
    pub task_key: TaskKey,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<Result<Envelope, ErrorOf<RecvErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct RecvClose {
    pub task_key: TaskKey,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Fork<R> {
    pub task_key: TaskKey,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<Result<Context<R>, ErrorOf<ForkErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct ForkRun {
    pub task_key:      TaskKey,
    pub address_lease: Option<AddressLease>,

    #[debug(skip)]
    pub(crate) task_fut: BoxFuture<'static, ()>,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct PendingTask {
    pub task_key:        TaskKey,
    pub address_lease:   Option<AddressLease>,
    #[debug(skip)]
    pub(crate) task_fut: BoxFuture<'static, ()>,
}

#[derive(derive_more::Debug)]
pub struct Quit {
    pub task_key: TaskKey,
    pub result:   Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>,

    #[debug(skip)]
    #[allow(dead_code)]
    pub(crate) outcome_tx: oneshot::Sender<Never>,
}

#[derive(derive_more::Debug)]
pub struct Tell {
    pub task_key: TaskKey,
    pub envelope: Envelope,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<Result<(), ErrorOf<TellErrorKind>>>,
}

#[derive(derive_more::Debug)]
pub struct Watch {
    pub task_key: TaskKey,
    pub peer:     Address,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<WatchRef>,
}

#[derive(derive_more::Debug)]
pub struct Unwatch {
    pub task_key:  TaskKey,
    pub watch_ref: WatchRef,

    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Link {
    pub task_key: TaskKey,

    pub peer:              Address,
    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Unlink {
    pub task_key: TaskKey,

    pub peer:              Address,
    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct SetTrapExit {
    pub task_key: TaskKey,

    pub enable:            bool,
    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<()>,
}

#[derive(derive_more::Debug)]
pub struct Exit {
    pub task_key: TaskKey,

    pub peer:              Address,
    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<bool>,
}

#[derive(derive_more::Debug)]
pub struct Kill {
    pub task_key: TaskKey,

    pub peer:              Address,
    #[debug(skip)]
    pub(crate) outcome_tx: oneshot::Sender<bool>,
}
