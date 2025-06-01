use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use mm1_common::errors::error_kind::HasErrorKind;
use mm1_common::log;
use mm1_core::context::{
    Fork, ForkErrorKind, InitDone, Linking, Messaging, Now, Quit, RecvErrorKind, Start, Stop,
    Watching,
};
use mm1_core::envelope::dispatch;
use mm1_proto::Message;
use mm1_proto_system::Exited;

use crate::common::child_spec::ChildSpec;
use crate::mixed::decider::{Action, Decider};
use crate::mixed::strategy::RestartStrategy;
use crate::mixed::{ErasedActorFactory, MixedSup, spec_builder, sup_child};

pub async fn mixed_sup<Runnable, Ctx, RS, CS, K>(
    ctx: &mut Ctx,
    sup_spec: MixedSup<RS, CS>,
) -> Result<(), MixedSupError>
where
    Runnable: Send,
    Ctx: Now<Instant = tokio::time::Instant>,
    Ctx: Fork + Messaging + Quit + InitDone + Linking + Watching + Stop + Start<Runnable>,
    CS: spec_builder::CollectInto<K, Runnable>,
    RS: RestartStrategy<K>,
    K: fmt::Display,
    K: Clone + Hash + Eq,
    K: Message,
    ChildSpec<ErasedActorFactory<Runnable>>: Send + Sync + 'static,
{
    ctx.set_trap_exit(true).await;

    let MixedSup {
        restart_strategy,
        children,
    } = sup_spec;
    let sup_addr = ctx.address();
    let mut decider = restart_strategy.decider();
    let children = do_init_children(&mut decider, children)?;

    loop {
        if let Some(action) = decider
            .next_action(ctx.now())
            .map_err(MixedSupError::decider)?
        {
            log::debug!("processing decider action: {}", action);
            match action {
                Action::Noop => (),
                Action::Quit { normal_exit } => {
                    if normal_exit {
                        ctx.quit_ok().await;
                    } else {
                        ctx.quit_err(MixedSupError::Escalated).await;
                    }
                },
                Action::Start { child_id } => {
                    let child_id = child_id.clone();
                    let child_spec = children
                        .get(&child_id)
                        .expect("child_id provided by the decider")
                        .clone()
                        .map_launcher(|f| f.produce(()));

                    let forked = ctx
                        .fork()
                        .await
                        .map_err(|e| e.kind())
                        .map_err(MixedSupError::Fork)?;
                    forked
                        .run(move |mut ctx| async move { sup_child::run(&mut ctx, sup_addr, child_id, child_spec).await })
                        .await;
                },

                Action::Stop { address, child_id } => {
                    let stop_timeout = child_id
                        .map(|id| {
                            children
                                .get(id)
                                .expect("child_id provided by the decider")
                                .stop_timeout
                        })
                        .unwrap_or_default();
                    let forked = ctx
                        .fork()
                        .await
                        .map_err(|e| e.kind())
                        .map_err(MixedSupError::Fork)?;
                    forked
                        .run(move |mut ctx| {
                            async move {
                                sup_child::shutdown(&mut ctx, sup_addr, address, stop_timeout).await
                            }
                        })
                        .await;
                },
            }
        }

        let received = ctx.recv().await.map_err(MixedSupError::recv)?;

        dispatch!(match received {
            sup_child::Started::<K> { child_id, address } => {
                log::debug!("[{}] started as {}. Linking...", child_id, address);
                ctx.link(address).await;
                decider.started(&child_id, address, ctx.now())
            },
            sup_child::StartFailed::<K> { child_id } => {
                log::warn!("failed to start [{}]. Initiating shutdown...", child_id);
                decider.failed(&child_id, ctx.now());
            },
            sup_child::StopFailed { address, reason } => {
                log::warn!(
                    "failed to stop {}: {}. Initiating shutdown...",
                    address,
                    reason
                );
                decider.quit(false);
            },

            Exited { peer, normal_exit } => {
                log::debug!("{} exited", peer);
                decider.exited(peer, normal_exit, ctx.now());
            },

            unexpected @ _ => log::warn!("unexpected message: {:?}", unexpected),
        });
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MixedSupError {
    #[error("escalated supervisor failure")]
    Escalated,

    #[error("decider: {}", _0)]
    Decider(String),

    #[error("recv: {}", _0)]
    Recv(RecvErrorKind),

    #[error("fork: {}", _0)]
    Fork(ForkErrorKind),
}

impl MixedSupError {
    pub fn decider(reason: impl fmt::Display) -> Self {
        Self::Decider(reason.to_string())
    }

    pub fn recv(reason: impl HasErrorKind<RecvErrorKind>) -> Self {
        Self::Recv(reason.kind())
    }
}

fn do_init_children<CS, D, R, K>(
    decider: &mut D,
    children: CS,
) -> Result<HashMap<K, ChildSpec<ErasedActorFactory<R>>>, MixedSupError>
where
    CS: spec_builder::CollectInto<K, R>,
    D: Decider<Key = K>,
    K: Clone + Hash + Eq,
{
    let mut flat = vec![];
    children.collect_into(&mut flat);

    for (k, _) in flat.iter() {
        decider.add(k.clone()).map_err(MixedSupError::decider)?;
    }

    let children_map = flat.into_iter().collect();

    Ok(children_map)
}
