use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::types::Never;
use mm1_core::context::{
    Fork, ForkErrorKind, InitDone, Linking, Now, Quit, Recv, RecvErrorKind, Start, Stop, Tell,
    TellErrorKind, Watching,
};
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_proto_system::{SpawnErrorKind, StartErrorKind, WatchRef};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::query::{self, Query};

pub struct TestContext<R> {
    pub(crate) tx:              mpsc::UnboundedSender<(Address, Query<R>)>,
    pub(crate) actor_address:   Address,
    pub(crate) context_address: Address,
    pub(crate) inbox_closed:    bool,
}

impl<R> Now for TestContext<R>
where
    R: Send + 'static,
{
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }
}

impl<R> Recv for TestContext<R>
where
    R: Send + 'static,
{
    fn address(&self) -> Address {
        self.context_address
    }

    fn close(&mut self) -> impl Future<Output = ()> + Send {
        self.inbox_closed = true;
        std::future::ready(())
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        if self.inbox_closed {
            return Err(ErrorOf::new(RecvErrorKind::Closed, "inbox closed"))
        }

        self.invoke(
            |outcome_tx| Query::Recv(query::Recv { outcome_tx }),
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> InitDone for TestContext<R>
where
    R: Send,
{
    fn init_done(&mut self, address: Address) -> impl Future<Output = ()> + Send {
        self.invoke(
            move |outcome_tx| {
                Query::InitDone(query::InitDone {
                    address,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }
}

impl<R> Watching for TestContext<R>
where
    Query<R>: Send,
{
    fn watch(&mut self, peer: Address) -> impl Future<Output = WatchRef> + Send {
        let this = self.context_address;
        self.invoke(
            move |outcome_tx| {
                Query::Watch(query::Watch {
                    this,
                    peer,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }

    fn unwatch(&mut self, watch_ref: WatchRef) -> impl Future<Output = ()> + Send {
        let this = self.context_address;
        self.invoke(
            move |outcome_tx| {
                Query::Unwatch(query::Unwatch {
                    this,
                    watch_ref,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }
}

impl<R> Tell for TestContext<R>
where
    R: Send + 'static,
{
    fn tell<M>(
        &mut self,
        to: Address,
        msg: M,
    ) -> impl Future<Output = Result<(), ErrorOf<TellErrorKind>>> + Send
    where
        M: mm1_core::prim::Message,
    {
        let info = EnvelopeInfo::new(to);
        let envelope = Envelope::new(info, msg).into_erased();
        self.invoke(
            move |outcome_tx| {
                Query::Tell(query::Tell {
                    envelope,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }
}

impl<R> Fork for TestContext<R>
where
    R: Send + 'static,
{
    fn fork(&mut self) -> impl Future<Output = Result<Self, ErrorOf<ForkErrorKind>>> + Send {
        self.invoke(
            |outcome_tx| Query::Fork(query::Fork { outcome_tx }),
            OnRxFailure::Panic,
        )
    }

    async fn run<F, Fut>(self, fun: F)
    where
        F: FnOnce(Self) -> Fut + Send,
        F: Send + 'static,
        Fut: Future + Send + 'static,
    {
        let actor_address = self.actor_address;
        let tx = self.tx.clone();
        let task = async move {
            let _ = fun(self).await;
        }
        .boxed();
        let (outcome_tx, outcome_rx) = oneshot::channel();
        let query = Query::Run(query::Run { task, outcome_tx });
        tx.send((actor_address, query)).expect("tx failed");
        outcome_rx.await.expect("rx failed")
    }
}

impl<R> Linking for TestContext<R>
where
    R: Send + 'static,
{
    fn link(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.context_address;
        self.invoke(
            move |outcome_tx| {
                Query::Link(query::Link {
                    this,
                    peer,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }

    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send {
        let this = self.context_address;
        self.invoke(
            move |outcome_tx| {
                Query::Unlink(query::Unlink {
                    this,
                    peer,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }

    fn set_trap_exit(&mut self, enable: bool) -> impl Future<Output = ()> + Send {
        self.invoke(
            move |outcome_tx| Query::SetTrapExit(query::SetTrapExit { enable, outcome_tx }),
            OnRxFailure::Panic,
        )
    }
}

impl<R> Quit for TestContext<R>
where
    R: Send + 'static,
{
    fn quit_ok(&mut self) -> impl Future<Output = Never> + Send {
        self.invoke(
            |outcome_tx| {
                Query::QuitOk(query::QuitOk {
                    _outcome_tx: outcome_tx,
                })
            },
            OnRxFailure::Freeze,
        )
    }

    fn quit_err<E>(&mut self, reason: E) -> impl Future<Output = Never> + Send
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.invoke(
            move |outcome_tx| {
                Query::QuitErr(query::QuitErr {
                    reason:      reason.into(),
                    _outcome_tx: outcome_tx,
                })
            },
            OnRxFailure::Freeze,
        )
    }
}

impl<R> Stop for TestContext<R>
where
    R: Send + 'static,
{
    fn exit(&mut self, peer: Address) -> impl Future<Output = bool> + Send {
        self.invoke(
            move |outcome_tx| Query::Exit(query::Exit { peer, outcome_tx }),
            OnRxFailure::Panic,
        )
    }

    fn kill(&mut self, peer: Address) -> impl Future<Output = bool> + Send {
        self.invoke(
            move |outcome_tx| Query::Kill(query::Kill { peer, outcome_tx }),
            OnRxFailure::Panic,
        )
    }
}

impl<R> Start<R> for TestContext<R>
where
    R: Send,
{
    fn spawn(
        &mut self,
        runnable: R,
        link: bool,
    ) -> impl Future<Output = Result<Address, ErrorOf<SpawnErrorKind>>> + Send {
        self.invoke(
            move |outcome_tx| {
                Query::Spawn(query::Spawn {
                    runnable,
                    link,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }

    fn start(
        &mut self,
        runnable: R,
        link: bool,
        start_timeout: Duration,
    ) -> impl Future<Output = Result<Address, ErrorOf<StartErrorKind>>> + Send {
        self.invoke(
            move |outcome_tx| {
                Query::Start(query::Start {
                    runnable,
                    link,
                    start_timeout,
                    outcome_tx,
                })
            },
            OnRxFailure::Panic,
        )
    }
}

impl<R> TestContext<R> {
    async fn invoke<F, Out>(&mut self, make_query: F, on_rx_failure: OnRxFailure) -> Out
    where
        F: FnOnce(oneshot::Sender<Out>) -> Query<R>,
    {
        let (outcome_tx, outcome_rx) = oneshot::channel();
        let query = make_query(outcome_tx);
        self.tx
            .send((self.actor_address, query))
            .expect("tx failed");
        match (outcome_rx.await, on_rx_failure) {
            (Ok(ret), _) => ret,
            (Err(reason), OnRxFailure::Panic) => panic!("rx failed: {}", reason),
            (Err(_), OnRxFailure::Freeze) => std::future::pending().await,
        }
    }
}

enum OnRxFailure {
    Panic,
    Freeze,
}
