use std::future::Future;
use std::pin::pin;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proc_macros::dispatch;
use mm1_proto_system::{self as system, Down, System};
use tracing::warn;

use super::{ForkErrorKind, Recv, RecvErrorKind};
use crate::context::{Call, Fork, Watching};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShutdownErrorKind {
    InternalError,
    Fork(ForkErrorKind),
    Recv(RecvErrorKind),
}

pub trait Stop<Sys>:
    Call<Sys, system::Exit, Outcome = bool> + Call<Sys, system::Kill, Outcome = bool>
where
    Sys: System,
{
    fn shutdown(
        &mut self,
        peer: Address,
        stop_timeout: Duration,
    ) -> impl Future<Output = Result<(), ErrorOf<ShutdownErrorKind>>> + Send
    where
        Sys: Default,
        Self: Watching<Sys> + Fork + Recv,
    {
        async move {
            let mut fork = self
                .fork()
                .await
                .map_err(|e| e.map_kind(ShutdownErrorKind::Fork))?;

            let watch_ref = fork.watch(peer).await;

            let mut shutdown_sequence = pin!(async {
                self.exit(peer).await;
                tokio::time::sleep(stop_timeout).await;
                self.kill(peer).await;
            }
            .fuse());

            let mut recv_result = pin!(async {
                loop {
                    dispatch!(match fork
                        .recv()
                        .await
                        .map_err(|e| e.map_kind(ShutdownErrorKind::Recv))?
                    {
                        down @ Down { .. } if down.watch_ref == watch_ref && down.peer == peer => {
                            break Ok(())
                        },

                        unexpected @ _ => {
                            warn!("unexpected message: {:?}", unexpected);
                        },
                    })
                }
            }
            .fuse());

            loop {
                tokio::select! {
                    recv_result = recv_result.as_mut() => { break recv_result },
                    () = shutdown_sequence.as_mut() => {}
                }
            }
        }
    }

    fn exit(&mut self, peer: Address) -> impl Future<Output = bool> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::Exit { peer }).await }
    }

    fn kill(&mut self, peer: Address) -> impl Future<Output = bool> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::Kill { peer }).await }
    }
}

impl<Sys, T> Stop<Sys> for T
where
    T: Call<Sys, system::Exit, Outcome = bool> + Call<Sys, system::Kill, Outcome = bool>,
    Sys: System,
{
}

impl_error_kind!(ShutdownErrorKind);
