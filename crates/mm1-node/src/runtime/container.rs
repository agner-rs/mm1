use std::collections::{BTreeSet, HashMap, HashSet};
use std::pin::pin;
use std::sync::Arc;

use either::Either;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use mm1_address::address::Address;
use mm1_address::pool::{Lease as AddressLease, LeaseError, Pool as SubnetPool};
use mm1_address::subnet::NetMask;
use mm1_common::futures::catch_panic::CatchPanicExt;
use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_proto::AnyError;
use mm1_proto_system::{self as system};
use tracing::{instrument, trace};

use super::config::{EffectiveActorConfig, Mm1Config};
use crate::runtime::actor_key::ActorKey;
use crate::runtime::rt_api::{RequestAddressError, RtApi};
use crate::runtime::runnable::{ActorRun, BoxedRunnable};
use crate::runtime::sys_call::{self, SysCall};
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg, SysWatch};
use crate::runtime::{context, mq};

pub(crate) struct ContainerArgs {
    pub(crate) ack_to:    Option<Address>,
    pub(crate) link_to:   Vec<Address>,
    pub(crate) actor_key: ActorKey,

    pub(crate) subnet_lease: AddressLease,
    pub(crate) rt_api:       RtApi,
    pub(crate) rt_config:    Arc<Mm1Config>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ContainerError {
    #[error("end of call-rx stream: an actor somehow managed to drop its context?")]
    EndOfCallRx,

    #[error("end of sys-msg-rx stream")]
    EndOfSysMsgRx,

    #[error("request address error")]
    RequestAddressError(#[source] RequestAddressError),

    #[error("lease error")]
    LeaseError(#[source] LeaseError),
}

struct JobEntry {
    linked_to:   HashSet<Address>,
    watches:     BTreeSet<(Address, system::WatchRef)>,
    watched_by:  BTreeSet<(Address, system::WatchRef)>,
    tx_priority: mq::UbTxWeak<Envelope>,
}

#[derive(Debug, thiserror::Error)]
#[error("killed")]
struct Killed;

#[derive(Debug, thiserror::Error)]
#[error("collateral of {}", _0)]
struct Collateral(Address);

#[derive(Debug, thiserror::Error)]
#[error("terminated by {}", _0)]
struct Terminated(Address);

#[derive(Debug, thiserror::Error)]
#[error("panic: {}", _0)]
struct Panic(Box<str>);

pub(crate) struct Container {
    ack_to:     Option<Address>,
    link_to:    Vec<Address>,
    actor_key:  ActorKey,
    inbox_size: usize,

    actor_subnet_lease:  AddressLease,
    actor_subnet:        SubnetPool,
    actor_address_lease: AddressLease,

    rt_api:    RtApi,
    rt_config: Arc<Mm1Config>,

    runnable: BoxedRunnable<context::ActorContext>,
}

impl Container {
    pub(crate) fn create(
        args: ContainerArgs,
        runnable: BoxedRunnable<context::ActorContext>,
    ) -> Result<Self, ContainerError> {
        let ContainerArgs {
            ack_to,
            link_to,
            actor_key,

            subnet_lease,
            rt_api,
            rt_config,
        } = args;
        let inbox_size = rt_config.actor_config(&actor_key).inbox_size();
        let actor_subnet = SubnetPool::new(subnet_lease.net_address());
        let actor_address = actor_subnet
            .lease(NetMask::M_64)
            .map_err(ContainerError::LeaseError)?;
        let container = Self {
            ack_to,
            link_to,
            actor_key,
            inbox_size,
            actor_subnet_lease: subnet_lease,
            actor_address_lease: actor_address,
            actor_subnet,
            rt_api,
            rt_config,
            runnable,
        };
        Ok(container)
    }

    pub(crate) fn actor_address(&self) -> Address {
        debug_assert_eq!(self.actor_address_lease.mask, NetMask::M_64);
        self.actor_address_lease.address
    }

    #[instrument(skip_all, fields(
        addr = display(&self.actor_address_lease.address),
        subn = display(&*self.actor_subnet_lease),
        func = self.runnable.func_name(),
        akey = display(&self.actor_key),
    ))]
    pub(crate) async fn run(self) -> Result<(AddressLease, Result<(), AnyError>), ContainerError> {
        // TODO: produce a future, that is instrumented
        let Self {
            ack_to,
            link_to,
            actor_key,
            inbox_size,
            actor_subnet_lease,
            actor_subnet,
            actor_address_lease,
            rt_api,
            rt_config,
            runnable,
        } = self;

        trace!("starting up");

        let (tx_system, mut rx_system) = mq::unbounded();
        let (tx_priority, rx_priority) = mq::unbounded();
        let (tx_regular, rx_regular) = mq::bounded(inbox_size);
        let (call_tx, call_rx) = sys_call::create();
        // let tx_regular_weak = tx_regular.downgrade();
        let tx_priority_weak = tx_priority.downgrade();
        let tx_system_weak = tx_system.downgrade();

        let actor_address = actor_address_lease.address;
        let () = rt_api.register(actor_address_lease, tx_system, tx_priority, tx_regular);

        let mut next_watch_ref: u64 = 0;
        let mut taken_watch_refs: HashMap<system::WatchRef, Address> = Default::default();

        let linked_to: HashSet<_> = link_to.into_iter().collect();
        {
            let tx_system = tx_system_weak
                .upgrade()
                .expect("come on! it's our own tx_system!");
            for peer in linked_to.iter().copied() {
                let sys_send_result = rt_api.sys_send(
                    peer,
                    SysMsg::Link(SysLink::Connect {
                        sender:   actor_address,
                        receiver: peer,
                    }),
                );
                if sys_send_result.is_err() {
                    let _ = tx_system.send(SysMsg::Link(SysLink::Exit {
                        sender:   peer,
                        receiver: actor_address,
                        reason:   ExitReason::LinkDown,
                    }));
                }
            }
        }

        let mut job_entries: HashMap<Address, JobEntry> = [(
            actor_address,
            JobEntry {
                tx_priority: tx_priority_weak,
                watches: Default::default(),
                watched_by: Default::default(),
                linked_to,
            },
        )]
        .into_iter()
        .collect();
        let mut trap_exit = false;

        let mut context = context::ActorContext {
            rt_api: rt_api.clone(),
            rt_config,
            actor_address,
            call: call_tx,
            rx_priority,
            rx_regular,
            tx_system_weak: tx_system_weak.clone(),
            subnet_pool: actor_subnet,
            actor_key,
            ack_to,
            unregister_on_drop: false,
        };

        let exit_reason = {
            let mut spawned_jobs = FuturesUnordered::new();
            let running = runnable.run(&mut context).catch_panic();
            let mut running = pin!(running.fuse());
            let mut call_rx = pin!(call_rx.fuse());

            loop {
                let sys_msg_recv = rx_system.recv().fuse();
                let call_next = call_rx.next();

                let spawn_jobs_non_empty = !spawned_jobs.is_empty();
                let spawn_job_next = spawned_jobs.next();

                let selected = tokio::select! {
                    biased;

                    call = call_next =>
                        Either::Right(
                        call
                            .ok_or(ContainerError::EndOfCallRx)
                            .inspect(|call| trace!("call::{}", call))
                            .inspect_err(|err| trace!("err: {}", err))?)
                        ,

                    sys_msg = sys_msg_recv =>

                        Either::Left(
                        sys_msg
                            .ok_or(ContainerError::EndOfSysMsgRx)
                            .inspect(|sys_msg| trace!("inbound sys-msg::{}", sys_msg))
                            .inspect_err(|err| trace!("err: {}", err))?),

                    output = running.as_mut() => {
                        let panic = output.expect_err("have we produced an instance of `std::convert::Infallible`?");
                        trace!("panic: {}", panic);
                        Either::Right(SysCall::Exit(Err(Panic(panic).into())))
                    },

                    _ = spawn_job_next, if spawn_jobs_non_empty => continue,
                };

                match (trap_exit, selected) {
                    (_, Either::Right(SysCall::Exit(exit_reason))) => {
                        break exit_reason;
                    },
                    (_, Either::Left(SysMsg::Kill)) => {
                        break Err(Killed.into());
                    },

                    (_, Either::Right(SysCall::TrapExit(set_into))) => {
                        trap_exit = set_into;
                    },

                    (_, Either::Right(SysCall::Spawn(job))) => {
                        spawned_jobs.push(job);
                    },

                    (_, Either::Right(SysCall::ForkAdded(fork_address, tx_priority))) => {
                        assert!(job_entries
                            .insert(
                                fork_address,
                                JobEntry {
                                    linked_to: Default::default(),
                                    watched_by: Default::default(),
                                    watches: Default::default(),
                                    tx_priority,
                                }
                            )
                            .is_none());
                    },

                    (_, Either::Left(SysMsg::ForkDone(fork_address))) => {
                        let JobEntry { linked_to, .. } = job_entries
                            .remove(&fork_address.address)
                            .expect("unknown fork");
                        for peer in linked_to {
                            let _ = rt_api.sys_send(
                                peer,
                                SysMsg::Link(SysLink::Disconnect {
                                    sender:   fork_address.address,
                                    receiver: peer,
                                }),
                            );
                        }
                    },

                    (_, Either::Right(SysCall::Link { sender, receiver })) => {
                        let job_entry = job_entries
                            .get_mut(&sender)
                            .expect("no job entry for this caller");
                        // for a newly linked peer
                        if job_entry.linked_to.insert(receiver) {
                            // send a SysLink:::Connect system-message
                            let sys_send_result = rt_api.sys_send(
                                receiver,
                                SysMsg::Link(SysLink::Connect { sender, receiver }),
                            );
                            // if sending a message has failed — treat it as peer's failure
                            if sys_send_result.is_err() {
                                let _ = tx_system_weak
                                    .upgrade()
                                    .expect("come on! it's our own tx_system!")
                                    .send(SysMsg::Link(SysLink::Exit {
                                        sender:   receiver,
                                        receiver: sender,
                                        reason:   ExitReason::LinkDown,
                                    }));
                            }
                        }
                    },

                    (_, Either::Left(SysMsg::Link(SysLink::Connect { sender, receiver }))) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            let _ = job_entry.linked_to.insert(sender);
                        } else {
                            let _ = rt_api.sys_send(
                                sender,
                                SysMsg::Link(SysLink::Exit {
                                    sender:   receiver,
                                    receiver: sender,
                                    reason:   ExitReason::LinkDown,
                                }),
                            );
                        }
                    },

                    (_, Either::Right(SysCall::Unlink { sender, receiver })) => {
                        let job_entry = job_entries
                            .get_mut(&sender)
                            .expect("no job entry for this caller");

                        // for a peer that we've previously linked to
                        if job_entry.linked_to.remove(&receiver) {
                            let _sys_send_result = rt_api.sys_send(
                                receiver,
                                SysMsg::Link(SysLink::Disconnect { sender, receiver }),
                            );
                        }
                    },

                    (
                        false,
                        Either::Left(SysMsg::Link(SysLink::Exit {
                            sender,
                            receiver,
                            reason: ExitReason::Normal,
                        })),
                    )
                    | (_, Either::Left(SysMsg::Link(SysLink::Disconnect { sender, receiver }))) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            let _ = job_entry.linked_to.remove(&sender);
                        }
                    },

                    (
                        false,
                        Either::Left(SysMsg::Link(SysLink::Exit {
                            sender,
                            receiver,
                            reason: ExitReason::LinkDown,
                        })),
                    ) => {
                        if job_entries
                            .get_mut(&receiver)
                            .map_or(false, |job_entry| job_entry.linked_to.remove(&sender))
                        {
                            break Err(Collateral(sender).into());
                        }
                    },

                    (
                        false,
                        Either::Left(SysMsg::Link(SysLink::Exit {
                            sender,
                            receiver,
                            reason: ExitReason::Terminate,
                        })),
                    ) => {
                        if job_entries.contains_key(&receiver) {
                            break Err(Terminated(sender).into());
                        }
                    },

                    (
                        true,
                        Either::Left(SysMsg::Link(SysLink::Exit {
                            sender,
                            receiver,
                            reason,
                        })),
                    ) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            let should_handle = match reason {
                                ExitReason::Terminate => true,
                                ExitReason::LinkDown | ExitReason::Normal => {
                                    job_entry.linked_to.remove(&sender)
                                },
                            };

                            if should_handle {
                                if let Some(tx_priority) = job_entry.tx_priority.upgrade() {
                                    let message = system::Exited {
                                        peer:        sender,
                                        normal_exit: matches!(reason, ExitReason::Normal),
                                    };
                                    let envelope =
                                        Envelope::new(EnvelopeInfo::new(receiver), message)
                                            .into_erased();
                                    let _ = tx_priority.send(envelope);
                                }
                            }
                        }
                    },

                    (
                        _,
                        Either::Right(SysCall::Watch {
                            sender,
                            receiver,
                            reply_tx,
                        }),
                    ) => {
                        let job_entry = job_entries
                            .get_mut(&sender)
                            .expect("no job entry for this caller");

                        let watch_ref = loop {
                            use std::collections::hash_map::Entry as HMEntry;

                            let candidate = next_watch_ref;
                            next_watch_ref = next_watch_ref.wrapping_add(1);
                            let watch_ref = system::WatchRef::from_u64(candidate);

                            if let HMEntry::Vacant(vacant) = taken_watch_refs.entry(watch_ref) {
                                vacant.insert(receiver);
                                break watch_ref
                            }
                        };

                        job_entry.watches.insert((receiver, watch_ref));

                        let sys_send_result = rt_api.sys_send(
                            receiver,
                            SysMsg::Watch(SysWatch::Watch {
                                sender,
                                receiver,
                                watch_ref,
                            }),
                        );

                        // if sending a message has failed — treat it as peer's failure
                        if sys_send_result.is_err() {
                            let _ = tx_system_weak
                                .upgrade()
                                .expect("come on! it's our own tx_system!")
                                .send(SysMsg::Watch(SysWatch::Down {
                                    sender:   receiver,
                                    receiver: sender,
                                    reason:   ExitReason::LinkDown,
                                }));
                        }

                        let _ = reply_tx.send(watch_ref);
                    },

                    (_, Either::Right(SysCall::Unwatch { sender, watch_ref })) => {
                        let job_entry = job_entries
                            .get_mut(&sender)
                            .expect("no job entry for this caller");

                        if let Some(receiver) = taken_watch_refs.remove(&watch_ref) {
                            job_entry.watches.remove(&(receiver, watch_ref));

                            let _ = rt_api.sys_send(
                                receiver,
                                SysMsg::Watch(SysWatch::Unwatch {
                                    sender,
                                    receiver,
                                    watch_ref,
                                }),
                            );
                        }
                    },

                    (
                        _,
                        Either::Left(SysMsg::Watch(SysWatch::Watch {
                            sender,
                            receiver,
                            watch_ref,
                        })),
                    ) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            job_entry.watched_by.insert((sender, watch_ref));
                        } else {
                            let _ = rt_api.sys_send(
                                sender,
                                SysMsg::Watch(SysWatch::Down {
                                    sender:   receiver,
                                    receiver: sender,
                                    reason:   ExitReason::LinkDown,
                                }),
                            );
                        }
                    },

                    (
                        _,
                        Either::Left(SysMsg::Watch(SysWatch::Unwatch {
                            sender,
                            receiver,
                            watch_ref,
                        })),
                    ) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            job_entry.watched_by.remove(&(sender, watch_ref));
                        }
                    },

                    (
                        _,
                        Either::Left(SysMsg::Watch(SysWatch::Down {
                            sender,
                            receiver,
                            reason,
                        })),
                    ) => {
                        if let Some(job_entry) = job_entries.get_mut(&receiver) {
                            let range =
                                (sender, system::WatchRef::MIN)..=(sender, system::WatchRef::MAX);
                            while let Some(to_remove) =
                                job_entry.watched_by.range(range.clone()).next().copied()
                            {
                                job_entry.watched_by.remove(&to_remove);
                            }
                            while let Some(to_report) =
                                job_entry.watches.range(range.clone()).next().copied()
                            {
                                job_entry.watches.remove(&to_report);
                                let (peer, watch_ref) = to_report;
                                taken_watch_refs.remove(&watch_ref);

                                if let Some(tx_priority) = job_entry.tx_priority.upgrade() {
                                    let message = system::Down {
                                        peer,
                                        watch_ref,
                                        normal_exit: matches!(reason, ExitReason::Normal),
                                    };
                                    let envelope =
                                        Envelope::new(EnvelopeInfo::new(receiver), message)
                                            .into_erased();
                                    let _ = tx_priority.send(envelope);
                                }
                            }
                        }
                    },
                }
            }
        };

        trace!("exitting [reason: {:?}]", exit_reason);

        let _ = rt_api.unregister(actor_address);

        trace!("processing the remaining sys-msgs");

        rx_system.close();
        while let Some(sys_msg) = rx_system.recv().await {
            trace!("sys-msg: {}", sys_msg);

            match sys_msg {
                SysMsg::Kill => (),
                SysMsg::ForkDone(fork_address) => {
                    let JobEntry {
                        linked_to,
                        watched_by,
                        ..
                    } = job_entries
                        .remove(&fork_address.address)
                        .expect("unknown fork");
                    for peer in linked_to {
                        let msg =
                            sys_link_exit_message(exit_reason.is_ok(), fork_address.address, peer);
                        let _ = rt_api.sys_send(peer, msg);
                    }
                    for peer in watched_by.into_iter().map(|(peer, _)| peer).filter_map({
                        let mut prev = None;
                        move |this| {
                            if prev.replace(this) == Some(this) {
                                None
                            } else {
                                Some(this)
                            }
                        }
                    }) {
                        let msg =
                            sys_watch_down_message(exit_reason.is_ok(), fork_address.address, peer);
                        let _ = rt_api.sys_send(peer, msg);
                    }
                },
                SysMsg::Link(SysLink::Disconnect { .. }) => (),
                SysMsg::Link(SysLink::Exit { .. }) => (),
                SysMsg::Link(SysLink::Connect { sender, receiver }) => {
                    let msg = sys_link_exit_message(exit_reason.is_ok(), receiver, sender);
                    let _ = rt_api.sys_send(sender, msg);
                },
                SysMsg::Watch(SysWatch::Unwatch { .. }) => (),
                SysMsg::Watch(SysWatch::Down { .. }) => (),
                SysMsg::Watch(SysWatch::Watch {
                    sender, receiver, ..
                }) => {
                    let msg = sys_watch_down_message(exit_reason.is_ok(), receiver, sender);
                    let _ = rt_api.sys_send(sender, msg);
                },
            }
        }
        let job_entry = job_entries
            .remove(&actor_address)
            .expect("main job-entry is gone somewhere");
        for peer in job_entry.linked_to {
            let msg = sys_link_exit_message(exit_reason.is_ok(), actor_address, peer);
            let _ = rt_api.sys_send(peer, msg);
        }
        for peer in job_entry
            .watched_by
            .into_iter()
            .map(|(peer, _)| peer)
            .filter_map({
                let mut prev = None;
                move |this| {
                    if prev.replace(this) == Some(this) {
                        None
                    } else {
                        Some(this)
                    }
                }
            })
        {
            let msg = sys_watch_down_message(exit_reason.is_ok(), actor_address, peer);
            let _ = rt_api.sys_send(peer, msg);
        }

        trace!("done");

        Ok((actor_subnet_lease, exit_reason))
    }
}

fn sys_link_exit_message(normal_exit: bool, sender: Address, receiver: Address) -> SysMsg {
    let sys_link = SysLink::Exit {
        sender,
        receiver,
        reason: if normal_exit {
            ExitReason::Normal
        } else {
            ExitReason::LinkDown
        },
    };
    SysMsg::Link(sys_link)
}

fn sys_watch_down_message(normal_exit: bool, sender: Address, receiver: Address) -> SysMsg {
    let sys_watch = SysWatch::Down {
        sender,
        receiver,
        reason: if normal_exit {
            ExitReason::Normal
        } else {
            ExitReason::LinkDown
        },
    };
    SysMsg::Watch(sys_watch)
}

impl From<RequestAddressError> for ContainerError {
    fn from(value: RequestAddressError) -> Self {
        Self::RequestAddressError(value)
    }
}
impl From<LeaseError> for ContainerError {
    fn from(value: LeaseError) -> Self {
        Self::LeaseError(value)
    }
}
