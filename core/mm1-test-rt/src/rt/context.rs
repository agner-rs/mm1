use futures::FutureExt;
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::types::Never;
use mm1_core::context::{
    Bind, BindArgs, BindErrorKind, Fork, ForkErrorKind, InitDone, Linking, Messaging, Now, Quit,
    RecvErrorKind, Start, Stop, Watching,
};
use mm1_core::envelope::Envelope;
use mm1_proto_system::{SpawnErrorKind, StartErrorKind};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::rt::{Query, TaskKey, TestContext, query};

impl TaskKey {
    pub fn actor(address: Address) -> Self {
        Self {
            actor:   address,
            context: address,
        }
    }

    pub fn fork(actor: Address, fork: Address) -> Self {
        assert_ne!(actor, fork);
        Self {
            actor,
            context: fork,
        }
    }
}

impl<R> Now for TestContext<R>
where
    R: Send,
{
    type Instant = Instant;

    fn now(&self) -> Self::Instant {
        Instant::now()
    }
}

impl<R> Start<R> for TestContext<R>
where
    R: Send,
{
    async fn spawn(&mut self, runnable: R, link: bool) -> Result<Address, ErrorOf<SpawnErrorKind>> {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Spawn {
                    task_key,
                    runnable,
                    link,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn start(
        &mut self,
        runnable: R,
        link: bool,
        start_timeout: std::time::Duration,
    ) -> Result<Address, ErrorOf<StartErrorKind>> {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Start {
                    task_key,
                    runnable,
                    link,
                    start_timeout,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Bind<NetAddress> for TestContext<R>
where
    R: Send + 'static,
{
    async fn bind(&mut self, args: BindArgs<NetAddress>) -> Result<(), ErrorOf<BindErrorKind>> {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            |outcome_tx| {
                query::Bind {
                    task_key,
                    args,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Fork for TestContext<R>
where
    R: Send + 'static,
{
    async fn fork(&mut self) -> Result<Self, ErrorOf<ForkErrorKind>> {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            |outcome_tx| {
                query::Fork {
                    task_key,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn run<F, Fut>(mut self, fun: F)
    where
        F: FnOnce(Self) -> Fut,
        F: Send + 'static,
        Fut: Future + Send + 'static,
    {
        let queries_tx = self.queries_tx.clone();
        let task_key = self.task_key;
        let address_lease = self.address_lease.take();
        let task_fut = async move {
            let _ = fun(self).await;
        }
        .boxed();
        invoke(
            &queries_tx,
            |outcome_tx| {
                query::ForkRun {
                    task_key,
                    address_lease,
                    task_fut,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Quit for TestContext<R>
where
    R: Send,
{
    async fn quit_ok(&mut self) -> Never {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Quit {
                    task_key,
                    result: Ok(()),
                    outcome_tx,
                }
            },
            OnRxFailure::Freeze,
        )
        .await
    }

    async fn quit_err<E>(&mut self, reason: E) -> Never
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Quit {
                    task_key,
                    result: Err(Box::new(reason)),
                    outcome_tx,
                }
            },
            OnRxFailure::Freeze,
        )
        .await
    }
}

impl<R> Messaging for TestContext<R>
where
    R: Send,
{
    fn address(&self) -> Address {
        self.task_key.context
    }

    async fn close(&mut self) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::RecvClose {
                    task_key,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Recv {
                    task_key,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn send(
        &mut self,
        envelope: Envelope,
    ) -> Result<(), ErrorOf<mm1_core::context::SendErrorKind>> {
        let task_key = self.task_key;

        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Tell {
                    task_key,
                    envelope,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Watching for TestContext<R>
where
    R: Send,
{
    async fn watch(&mut self, peer: Address) -> mm1_proto_system::WatchRef {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Watch {
                    task_key,
                    peer,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn unwatch(&mut self, watch_ref: mm1_proto_system::WatchRef) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Unwatch {
                    task_key,
                    watch_ref,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Linking for TestContext<R>
where
    R: Send,
{
    async fn link(&mut self, peer: Address) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            |outcome_tx| {
                query::Link {
                    task_key,
                    peer,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn unlink(&mut self, peer: Address) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            |outcome_tx| {
                query::Unlink {
                    task_key,
                    peer,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn set_trap_exit(&mut self, enable: bool) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            |outcome_tx| {
                query::SetTrapExit {
                    task_key,
                    enable,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> InitDone for TestContext<R>
where
    R: Send,
{
    async fn init_done(&mut self, address: Address) {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::InitDone {
                    task_key,
                    address,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

impl<R> Stop for TestContext<R>
where
    R: Send,
{
    async fn exit(&mut self, peer: Address) -> bool {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Exit {
                    task_key,
                    peer,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }

    async fn kill(&mut self, peer: Address) -> bool {
        let task_key = self.task_key;
        invoke(
            &self.queries_tx,
            move |outcome_tx| {
                query::Kill {
                    task_key,
                    peer,
                    outcome_tx,
                }
            },
            OnRxFailure::Panic,
        )
        .await
    }
}

async fn invoke<R, F, Q, Out>(
    queries_tx: &mpsc::UnboundedSender<Query<R>>,
    make_query: F,
    on_rx_failure: OnRxFailure,
) -> Out
where
    F: FnOnce(oneshot::Sender<Out>) -> Q,
    Q: Into<Query<R>>,
{
    let (outcome_tx, outcome_rx) = oneshot::channel();
    let query = make_query(outcome_tx);
    queries_tx.send(query.into()).expect("tx failed");
    match (outcome_rx.await, on_rx_failure) {
        (Ok(ret), _) => ret,
        (Err(reason), OnRxFailure::Panic) => panic!("rx failed: {reason}"),
        (Err(_), OnRxFailure::Freeze) => std::future::pending().await,
    }
}

enum OnRxFailure {
    Panic,
    Freeze,
}
