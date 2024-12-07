use std::future::Future;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::futures::timeout::FutureTimeoutExt;
use mm1_proc_macros::dispatch;
use mm1_proto_system as system;
use mm1_proto_system::{Runnable, StartErrorKind, System};

use crate::context::call::Call;
use crate::context::fork::Fork;
use crate::context::recv::Recv;
use crate::context::tell::Tell;

pub trait Start<Sys>:
    Tell
    + Recv
    + Fork
    + Call<Sys, system::SpawnRequest<Sys>, Outcome = system::SpawnResponse>
    + Call<Sys, system::Kill, Outcome = bool>
    + Call<Sys, system::Link, Outcome = ()>
where
    Sys: System,
{
    fn spawn(
        &mut self,
        runnable: Sys::Runnable,
        link: bool,
    ) -> impl Future<Output = system::SpawnResponse> + Send {
        async move {
            let run_at = runnable.run_at();
            let link_to = if link { vec![self.address()] } else { vec![] };
            let spawn_request = system::SpawnRequest {
                runnable,
                ack_to: None,
                link_to,
            };
            self.call(run_at, spawn_request).await
        }
    }

    fn start(
        &mut self,
        runnable: Sys::Runnable,
        link: bool,
        start_timeout: Duration,
    ) -> impl Future<Output = Result<Address, ErrorOf<StartErrorKind>>> + Send {
        async move {
            let run_at = runnable.run_at();

            let mut fork = self
                .fork()
                .await
                .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?;
            let link_to = if link { vec![fork.address()] } else { vec![] };

            let spawn_request = system::SpawnRequest {
                runnable,
                ack_to: Some(fork.address()),
                link_to,
            };
            let spawned_address = fork
                .call(run_at, spawn_request)
                .await
                .map_err(|e| e.map_kind(StartErrorKind::Spawn))?;

            let envelope = match fork.recv().timeout(start_timeout).await {
                Err(_elapsed) => {
                    self.call(
                        run_at,
                        system::Kill {
                            peer: spawned_address,
                        },
                    )
                    .await;

                    // TODO: should we ensure termination with a `system::Watch`?

                    return Err(ErrorOf::new(
                        StartErrorKind::Timeout,
                        "no init-ack within timeout",
                    ))
                },
                Ok(recv_result) => {
                    recv_result
                        .map_err(|e| ErrorOf::new(StartErrorKind::InternalError, e.to_string()))?
                },
            };

            dispatch!(match envelope {
                system::InitAck { address } => {
                    if link {
                        self.call(run_at, system::Link { peer: address }).await;
                    }
                    Ok(address)
                },

                system::Exited { .. } => {
                    Err(ErrorOf::new(
                        StartErrorKind::Exited,
                        "exited before init-ack",
                    ))
                },

                unexpected @ _ => {
                    Err(ErrorOf::new(
                        StartErrorKind::InternalError,
                        format!("unexpected message: {:?}", unexpected),
                    ))
                },
            })
        }
    }
}
pub trait InitDone<Sys>: Call<Sys, system::InitAck, Outcome = ()>
where
    Sys: System + Default,
{
    fn init_done(&mut self, address: Address) -> impl Future<Output = ()> + Send {
        async move { self.call(Sys::default(), system::InitAck { address }).await }
    }
}

impl<Sys, T> Start<Sys> for T
where
    T: Tell
        + Recv
        + Fork
        + Call<Sys, system::SpawnRequest<Sys>, Outcome = system::SpawnResponse>
        + Call<Sys, system::Kill, Outcome = bool>
        + Call<Sys, system::Link, Outcome = ()>,
    Sys: System,
{
}

impl<Sys, T> InitDone<Sys> for T
where
    T: Call<Sys, system::InitAck, Outcome = ()>,
    Sys: System + Default,
{
}
