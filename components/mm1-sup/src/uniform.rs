use std::collections::HashSet;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_ask::Reply;
use mm1_common::errors::error_kind::HasErrorKind;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, warn};
use mm1_core::context::{
    Fork, ForkErrorKind, InitDone, Linking, Messaging, Quit, RecvErrorKind, Start, Stop, Tell,
    Watching,
};
use mm1_core::envelope::dispatch;
use mm1_proto::message;
use mm1_proto_ask::Request;
use mm1_proto_sup::uniform::{self as unisup};
use mm1_proto_system::{
    StartErrorKind, StopErrorKind, {self as system},
};

use crate::common::child_spec::{ChildSpec, InitType};
use crate::common::factory::ActorFactory;

pub trait UniformSupContext<Runnable>:
    Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}

impl<Ctx, Runnable> UniformSupContext<Runnable> for Ctx where
    Ctx: Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}

#[derive(Debug, thiserror::Error)]
#[message]
pub enum UniformSupFailure {
    #[error("recv error: {}", _0)]
    Recv(RecvErrorKind),

    #[error("fork error: {}", _0)]
    Fork(ForkErrorKind),
}

pub struct UniformSup<F> {
    pub child_spec: ChildSpec<F>,
}

impl<F> UniformSup<F> {
    pub fn new(child_spec: ChildSpec<F>) -> Self {
        Self { child_spec }
    }
}

pub async fn uniform_sup<R, Ctx, F>(
    ctx: &mut Ctx,
    sup_spec: UniformSup<F>,
) -> Result<(), UniformSupFailure>
where
    R: Send + 'static,
    Ctx: UniformSupContext<R>,
    F: ActorFactory<Runnable = R>,
    F::Args: Send,
{
    let UniformSup { child_spec } = sup_spec;
    let ChildSpec {
        launcher: factory,
        child_type,
        init_type,
        stop_timeout,
    } = child_spec;

    let _ = child_type;

    ctx.set_trap_exit(true).await;
    ctx.init_done(ctx.address()).await;

    let sup_address = ctx.address();
    let mut started_children: HashSet<Address> = Default::default();
    let mut stopping_children: HashSet<Address> = Default::default();

    loop {
        dispatch!(match ctx.recv().await.map_err(UniformSupFailure::recv)? {
            Request::<_, ()> {
                header: reply_to,
                payload: unisup::StartRequest::<F::Args> { args },
            } => {
                debug!("start request [reply_to: {}]", reply_to);

                let runnable = factory.produce(args);
                ctx.fork()
                    .await
                    .map_err(UniformSupFailure::fork)?
                    .run(move |mut ctx| {
                        async move {
                            let result =
                                do_start_child(&mut ctx, sup_address, init_type, runnable).await;
                            let _ = ctx.reply(reply_to, result).await;
                        }
                    })
                    .await;
            },
            ChildStarted(child) => {
                ctx.link(child).await;
                assert!(started_children.insert(child));
            },

            Request::<_, ()> {
                header: reply_to,
                payload: unisup::StopRequest { child },
            } => {
                debug!("stop request [reply_to: {}; child: {}]", reply_to, child);

                if stopping_children.insert(child) {
                    ctx.fork()
                        .await
                        .map_err(UniformSupFailure::fork)?
                        .run(move |mut ctx| {
                            async move {
                                let result =
                                    do_stop_child(&mut ctx, sup_address, stop_timeout, child).await;
                                let _ = ctx.reply(reply_to, result).await;
                            }
                        })
                        .await;
                } else {
                    let _ = ctx
                        .reply(
                            reply_to,
                            unisup::StopResponse::Err(ErrorOf::new(
                                StopErrorKind::NotFound,
                                "not found",
                            )),
                        )
                        .await;
                }
            },

            system::Exited { peer, normal_exit } =>
                match (
                    started_children.remove(&peer),
                    stopping_children.remove(&peer),
                    normal_exit,
                ) {
                    (false, true, _) => unreachable!(),
                    (true, true, normal_exit) =>
                        debug!(
                            "a stopping child terminated [child: {}; normal_exit: {}]",
                            peer, normal_exit
                        ),
                    (true, false, normal_exit) =>
                        warn!(
                            "a child unexpectedly terminated [child: {}; normal_exit: {}]",
                            peer, normal_exit
                        ),
                    (false, false, true) => (),
                    (false, false, false) => {
                        // TODO: reap all the children before giving up
                        debug!(
                            "unknown linked process terminated. Exitting. [offender: {}]",
                            peer
                        );
                        ctx.quit_err(UnknownPeerExited(peer)).await;
                    },
                },

            any @ _ => {
                warn!("unexpected message: {:?}", any)
            },
        })
    }
}

async fn do_start_child<Runnable, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    init_type: InitType,
    runnable: Runnable,
) -> unisup::StartResponse
where
    Ctx: Messaging + Start<Runnable>,
{
    debug!("starting child [init_type: {:?}]", init_type,);

    let result = match init_type {
        InitType::NoAck => {
            ctx.spawn(runnable, true)
                .await
                .map_err(|e| e.map_kind(StartErrorKind::Spawn))
        },
        InitType::WithAck { start_timeout } => ctx.start(runnable, true, start_timeout).await,
    };
    match result {
        Err(reason) => {
            warn!("error [reason: {}]", reason);
            Err(reason)
        },
        Ok(child) => {
            debug!("child [address: {}]", child);
            let _ = ctx.tell(sup_address, ChildStarted(child)).await;
            Ok(child)
        },
    }
}

async fn do_stop_child<Ctx>(
    ctx: &mut Ctx,
    _sup_address: Address,
    stop_timeout: Duration,
    child_address: Address,
) -> unisup::StopResponse
where
    Ctx: Fork + Stop + Watching + Messaging,
{
    debug!(
        "stopping child [child_address: {}, stop_timeout: {:?}]",
        child_address, stop_timeout
    );

    ctx.shutdown(child_address, stop_timeout)
        .await
        .map_err(|e| e.map_kind(|_| StopErrorKind::InternalError))
}

#[derive(Debug)]
#[message]
struct ChildStarted(Address);

#[derive(Debug, thiserror::Error)]
#[error("unknown peer failure: {}", _0)]
struct UnknownPeerExited(Address);

impl UniformSupFailure {
    fn fork(e: impl HasErrorKind<ForkErrorKind> + Send) -> Self {
        Self::Fork(e.kind())
    }

    fn recv(e: impl HasErrorKind<RecvErrorKind> + Send) -> Self {
        Self::Recv(e.kind())
    }
}

impl<F> Clone for UniformSup<F>
where
    ChildSpec<F>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            child_spec: self.child_spec.clone(),
        }
    }
}
