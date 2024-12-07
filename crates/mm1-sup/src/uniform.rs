use std::collections::HashSet;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_kind::HasErrorKind;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, warn};
use mm1_core::context::{
    dispatch, Fork, ForkErrorKind, InitDone, Linking, Quit, Recv, RecvErrorKind, Start, Stop, Tell,
    Watching,
};
use mm1_proto::Traversable;
use mm1_proto_sup::uniform::{self as unisup};
use mm1_proto_system::{
    StartErrorKind, StopErrorKind, System, {self as system},
};

use crate::common::child_spec::{ChildSpec, ChildTimeouts, InitType};
use crate::common::factory::ActorFactory;

#[derive(Debug, thiserror::Error)]
pub enum UniformSupFailure {
    #[error("recv error: {}", _0)]
    Recv(RecvErrorKind),

    #[error("fork error: {}", _0)]
    Fork(ForkErrorKind),
}

pub struct UniformSup<F, D = Duration> {
    pub child_spec: ChildSpec<F, D>,
}

impl<F, D> UniformSup<F, D> {
    pub fn new(child_spec: ChildSpec<F, D>) -> Self {
        Self { child_spec }
    }
}

pub async fn uniform_sup<Sys, Ctx, F>(
    ctx: &mut Ctx,
    sup_spec: UniformSup<F, Duration>,
) -> Result<(), UniformSupFailure>
where
    Sys: System + Default,

    Ctx: Fork + Recv + Tell + Quit,
    Ctx: InitDone<Sys>,
    Ctx: Linking<Sys>,
    Ctx: Watching<Sys>,
    Ctx: Start<Sys>,
    Ctx: Stop<Sys>,
    F: ActorFactory<Runnable = Sys::Runnable>,
    F::Args: Send,
{
    let UniformSup { child_spec } = sup_spec;
    let ChildSpec {
        factory,
        child_type,
        init_type,
        timeouts,
    } = child_spec;
    let ChildTimeouts {
        start_timeout,
        stop_timeout,
    } = timeouts;

    let _ = child_type;
    let _ = timeouts;

    ctx.set_trap_exit(true).await;
    ctx.init_done(ctx.address()).await;

    let sup_address = ctx.address();
    let mut started_children: HashSet<Address> = Default::default();
    let mut stopping_children: HashSet<Address> = Default::default();

    loop {
        dispatch!(match ctx.recv().await.map_err(UniformSupFailure::recv)? {
            unisup::StartRequest::<F::Args> { reply_to, args } => {
                debug!("start request [reply_to: {}]", reply_to);

                let runnable = factory.produce(args);
                ctx.fork()
                    .await
                    .map_err(UniformSupFailure::fork)?
                    .run(move |mut ctx| {
                        async move {
                            let result = do_start_child(
                                &mut ctx,
                                sup_address,
                                init_type,
                                start_timeout,
                                runnable,
                            )
                            .await;
                            let _ = ctx.tell(reply_to, result).await;
                        }
                    })
                    .await;
            },
            ChildStarted(child) => {
                ctx.link(child).await;
                assert!(started_children.insert(child));
            },

            unisup::StopRequest { reply_to, child } => {
                debug!("stop request [reply_to: {}; child: {}]", reply_to, child);

                if stopping_children.insert(child) {
                    ctx.fork()
                        .await
                        .map_err(UniformSupFailure::fork)?
                        .run(move |mut ctx| {
                            async move {
                                let result =
                                    do_stop_child(&mut ctx, sup_address, stop_timeout, child).await;
                                let _ = ctx.tell(reply_to, result).await;
                            }
                        })
                        .await;
                } else {
                    let _ = ctx
                        .tell(
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

async fn do_start_child<Sys, Ctx>(
    ctx: &mut Ctx,
    sup_address: Address,
    init_type: InitType,
    start_timeout: Duration,
    runnable: Sys::Runnable,
) -> unisup::StartResponse
where
    Sys: System + Default,
    Ctx: Recv + Tell,
    Ctx: Start<Sys>,
{
    debug!(
        "starting child [init_type: {:?}, start_timeout: {:?}]",
        init_type, start_timeout
    );

    let result = match init_type {
        InitType::NoAck => {
            ctx.spawn(runnable, true)
                .await
                .map_err(|e| e.map_kind(StartErrorKind::Spawn))
        },
        InitType::WithAck => ctx.start(runnable, true, start_timeout).await,
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

async fn do_stop_child<Sys, Ctx>(
    ctx: &mut Ctx,
    _sup_address: Address,
    stop_timeout: Duration,
    child_address: Address,
) -> unisup::StopResponse
where
    Sys: System + Default,
    Ctx: Recv + Fork,
    Ctx: Stop<Sys> + Watching<Sys>,
{
    debug!(
        "stopping child [child_address: {}, stop_timeout: {:?}]",
        child_address, stop_timeout
    );

    ctx.shutdown(child_address, stop_timeout)
        .await
        .map_err(|e| e.map_kind(|_| StopErrorKind::InternalError))
}

#[derive(Debug, Traversable)]
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
