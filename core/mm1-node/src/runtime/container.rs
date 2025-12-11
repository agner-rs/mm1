use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;
use std::pin::pin;
use std::sync::Arc;

use either::Either;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use mm1_address::address::Address;
use mm1_address::pool::{Lease as AddressLease, LeaseError, Pool};
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_common::errors::chain::{ExactTypeDisplayChainExt, StdErrorDisplayChainExt};
use mm1_common::futures::catch_panic::CatchPanicExt;
use mm1_common::types::AnyError;
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_core::tracing::{TraceId, WithTraceIdExt};
use mm1_proto_system::{self as system};
use mm1_runnable::local::{ActorRun, BoxedRunnable};
use tokio::sync::mpsc;
use tracing::{instrument, trace};

use crate::actor_key::ActorKey;
use crate::config::{EffectiveActorConfig, Mm1NodeConfig, Valid};
use crate::registry::{self, MessageWithoutPermit, Node, SubnetMailboxRx, WeakSubnetMailboxTx};
use crate::runtime::context;
use crate::runtime::rt_api::{RequestAddressError, RtApi};
use crate::runtime::sys_call::{self, SysCall};
use crate::runtime::sys_msg::{ExitReason, SysLink, SysMsg, SysWatch};

pub(crate) struct ContainerArgs {
    pub(crate) ack_to:    Option<Address>,
    pub(crate) link_to:   Vec<Address>,
    pub(crate) actor_key: ActorKey,
    pub(crate) trace_id:  TraceId,

    pub(crate) subnet_lease: AddressLease,
    pub(crate) rt_api:       RtApi,
    pub(crate) rt_config:    Arc<Valid<Mm1NodeConfig>>,
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

    #[error("registration error")]
    RegistrationError,
}

struct JobEntry {
    linked_to:  HashSet<Address>,
    watches:    BTreeSet<(Address, system::WatchRef)>,
    watched_by: BTreeSet<(Address, system::WatchRef)>,
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
    ack_to:    Option<Address>,
    link_to:   Vec<Address>,
    actor_key: ActorKey,
    trace_id:  TraceId,

    actor_subnet:            NetAddress,
    actor_subnet_pool:       Pool,
    main_fork_address_lease: AddressLease,

    subnet_mailbox_tx: WeakSubnetMailboxTx<(TraceId, SysMsg), Envelope>,
    subnet_mailbox_rx: SubnetMailboxRx<(TraceId, SysMsg), Envelope>,

    rt_api:    RtApi,
    rt_config: Arc<Valid<Mm1NodeConfig>>,

    runnable: BoxedRunnable<context::ActorContext>,

    tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
}

impl Container {
    pub(crate) fn create(
        args: ContainerArgs,
        runnable: BoxedRunnable<context::ActorContext>,
        tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
    ) -> Result<Self, ContainerError> {
        let ContainerArgs {
            ack_to,
            link_to,
            actor_key,
            trace_id,

            subnet_lease,
            rt_api,
            rt_config,
        } = args;
        let inbox_size = rt_config.actor_config(&actor_key).inbox_size();

        let (subnet_mailbox_tx, subnet_mailbox_rx) = registry::new_mailbox();

        let actor_subnet = subnet_lease.net_address();
        let actor_node = Node::new(subnet_lease, inbox_size, subnet_mailbox_tx.clone());
        let () = rt_api
            .registry()
            .register(actor_subnet, actor_node)
            .map_err(|_| ContainerError::RegistrationError)?;

        let actor_subnet_pool = Pool::new(actor_subnet);
        let main_actor_address_lease = actor_subnet_pool
            .lease(NetMask::MAX)
            .map_err(ContainerError::LeaseError)?;

        let subnet_mailbox_tx = subnet_mailbox_tx.downgrade();

        let container = Self {
            ack_to,
            link_to,
            trace_id,
            actor_key,
            actor_subnet,
            actor_subnet_pool,
            main_fork_address_lease: main_actor_address_lease,
            subnet_mailbox_rx,
            subnet_mailbox_tx,
            rt_api,
            rt_config,
            runnable,
            tx_actor_failure,
        };
        Ok(container)
    }

    pub(crate) fn actor_address(&self) -> Address {
        self.main_fork_address_lease.address
    }

    #[instrument(skip_all, fields(
        _addr = display(&self.main_fork_address_lease.address),
        _subn = display(&self.actor_subnet),
        _func = self.runnable.func_name(),
        _akey = display(&self.actor_key),
    ))]
    pub(crate) async fn run(self) -> Result<Result<(), AnyError>, ContainerError> {
        let Self {
            ack_to,
            link_to,
            actor_key,
            trace_id,
            actor_subnet,
            actor_subnet_pool,
            main_fork_address_lease,
            subnet_mailbox_rx,
            subnet_mailbox_tx,
            rt_api,
            rt_config,
            runnable,
            tx_actor_failure,
        } = self;

        let main_fork_address = main_fork_address_lease.address;
        trace!(%main_fork_address, "starting up");

        let (call_tx, call_rx) = sys_call::create();

        let mut next_watch_ref: u64 = 0;
        let mut taken_watch_refs: HashMap<system::WatchRef, Address> = Default::default();

        let linked_to: HashSet<_> = link_to.into_iter().collect();
        {
            let tx_system = subnet_mailbox_tx
                .tx_system
                .upgrade()
                .expect("come on! it's our own tx_system!");
            for peer in linked_to.iter().copied() {
                let sys_send_result = rt_api.sys_send(
                    peer,
                    SysMsg::Link(SysLink::Connect {
                        sender:   main_fork_address,
                        receiver: peer,
                    }),
                );
                if sys_send_result.is_err() {
                    let sys_msg = SysMsg::Link(SysLink::Exit {
                        sender:   peer,
                        receiver: main_fork_address,
                        reason:   ExitReason::LinkDown,
                    });
                    let _ = tx_system.send((TraceId::current(), sys_msg));
                }
            }
        }

        let mut job_entries: HashMap<Address, JobEntry> = [(
            main_fork_address,
            JobEntry {
                watches: Default::default(),
                watched_by: Default::default(),
                linked_to,
            },
        )]
        .into_iter()
        .collect();
        let mut trap_exit = false;

        let SubnetMailboxRx {
            mut rx_system,
            subnet_notify,
            rx_priority,
            rx_regular,
        } = subnet_mailbox_rx;
        let subnet_context = context::SubnetContext {
            rt_api: rt_api.clone(),
            rt_config,
            actor_key,
            subnet_pool: actor_subnet_pool,
            subnet_address: actor_subnet,
            subnet_notify,
            rx_regular,
            rx_priority,
            subnet_mailbox_tx: subnet_mailbox_tx.clone(),
            fork_entries: [(main_fork_address, Default::default())]
                .into_iter()
                .collect(),
            bound_subnets: Default::default(),
            tx_actor_failure,
        };
        let mut context = context::ActorContext {
            fork_address: main_fork_address,
            fork_lease: Some(main_fork_address_lease),
            ack_to,
            call: call_tx,

            subnet_context: Arc::new(subnet_context.into()),
        };

        let exit_reason = {
            let mut spawned_jobs = FuturesUnordered::new();
            let running = runnable
                .run(&mut context)
                .with_trace_id(trace_id)
                .catch_panic();
            let mut running = pin!(running.fuse());
            let mut call_rx = pin!(call_rx.fuse());

            loop {
                trace!("enter loop");

                let sys_msg_recv = rx_system.recv().fuse();
                let call_next = call_rx.next();

                let spawn_jobs_non_empty = !spawned_jobs.is_empty();
                // XXX: These can panic too, can't they?
                let spawn_job_next = spawned_jobs.next();

                let selected = tokio::select! {
                    biased;

                    call = call_next =>
                        Either::Right(
                            call
                                .ok_or(ContainerError::EndOfCallRx)
                                .inspect(|(trace_id, call)| trace!(?call, associated_trace_id = %trace_id, "call"))
                                .inspect_err(|err| trace!(err = %err.as_display_chain(), "error"))?),

                    sys_msg = sys_msg_recv =>
                        Either::Left(
                            sys_msg
                                .ok_or(ContainerError::EndOfSysMsgRx)
                                .inspect(|(trace_id, sys_msg)| trace!(?sys_msg, associated_trace_id = %trace_id, "inbound sys-msg"))
                                .inspect_err(|err| trace!(err = %err.as_display_chain(), "error"))?),

                    output = running.as_mut() => {
                        let panic = output.expect_err("have we produced an instance of `std::convert::Infallible`?");
                        trace!(%panic, "main fork panicked");
                        Either::Right(
                            (
                                TraceId::current(),
                                SysCall::Exit(Err(Panic(panic).into()))
                            )
                        )
                    },

                    spawned_job_done = spawn_job_next, if spawn_jobs_non_empty => {
                        if let Err(panic) = spawned_job_done.transpose() {
                            trace!(%panic, "spawned job panicked");
                            Either::Right(
                                (
                                    TraceId::current(),
                                    SysCall::Exit(Err(Panic(panic).into()))
                                )
                            )
                        } else {
                            continue
                        }
                    },
                };

                let (trace_id, selected) = match selected {
                    Either::Left((trace_id, sys_msg)) => (trace_id, Either::Left(sys_msg)),
                    Either::Right((trace_id, sys_call)) => (trace_id, Either::Right(sys_call)),
                };

                trace!(?selected, "processing...");

                let cf = trace_id.scope_sync(|| {
                    match (trap_exit, selected) {
                        (_, Either::Right(SysCall::Exit(exit_reason))) => {
                            return ControlFlow::Break(exit_reason)
                        },
                        (_, Either::Left(SysMsg::Kill)) => {
                            return ControlFlow::Break(Err(Killed.into()))
                        },

                        (_, Either::Right(SysCall::TrapExit(set_into))) => {
                            trap_exit = set_into;
                        },

                        (_, Either::Right(SysCall::Spawn(job))) => {
                            spawned_jobs.push(job.catch_panic());
                        },

                        (_, Either::Right(SysCall::ForkAdded(fork_address))) => {
                            let should_be_none = job_entries.insert(
                                fork_address,
                                JobEntry {
                                    linked_to:  Default::default(),
                                    watched_by: Default::default(),
                                    watches:    Default::default(),
                                },
                            );
                            assert!(should_be_none.is_none());
                        },

                        (_, Either::Left(SysMsg::ForkDone(fork_address))) => {
                            let JobEntry {
                                linked_to,
                                watched_by,
                                ..
                            } = job_entries
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
                            for peer in watched_by.into_iter().map(|(peer, _)| peer).filter_map({
                                let mut prev = None;
                                move |this| {
                                    prev.replace(this)
                                        .is_none_or(|prev| prev != this)
                                        .then_some(this)
                                }
                            }) {
                                let msg = sys_watch_down_message(true, fork_address.address, peer);
                                let _ = rt_api.sys_send(peer, msg);
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
                                    let sys_msg = SysMsg::Link(SysLink::Exit {
                                        sender:   receiver,
                                        receiver: sender,
                                        reason:   ExitReason::LinkDown,
                                    });
                                    let _ = subnet_mailbox_tx
                                        .tx_system
                                        .upgrade()
                                        .expect("come on! it's our own tx_system!")
                                        .send((TraceId::current(), sys_msg));
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
                        | (
                            _,
                            Either::Left(SysMsg::Link(SysLink::Disconnect { sender, receiver })),
                        ) => {
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
                                .is_some_and(|job_entry| job_entry.linked_to.remove(&sender))
                            {
                                return ControlFlow::Break(Err(Collateral(sender).into()))
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
                                return ControlFlow::Break(Err(Terminated(sender).into()))
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
                            trace!(
                                %sender,
                                %receiver,
                                ?reason,
                                "processing Exit when trap_exit=true"
                            );
                            if let Some(job_entry) = job_entries.get_mut(&receiver) {
                                let should_handle = match reason {
                                    ExitReason::Terminate => true,
                                    ExitReason::LinkDown | ExitReason::Normal => {
                                        job_entry.linked_to.remove(&sender)
                                    },
                                };

                                if should_handle
                                    && let Some((tx_priority, subnet_notify)) = subnet_mailbox_tx
                                        .tx_priority
                                        .upgrade()
                                        .zip(subnet_mailbox_tx.subnet_notify.upgrade())
                                {
                                    let message = system::Exited {
                                        peer:        sender,
                                        normal_exit: matches!(reason, ExitReason::Normal),
                                    };
                                    let envelope = Envelope::new(
                                        EnvelopeHeader::to_address(receiver),
                                        message,
                                    )
                                    .into_erased();
                                    let _ = tx_priority.try_send(MessageWithoutPermit {
                                        to:      receiver,
                                        message: envelope,
                                    });
                                    subnet_notify.notify_one();
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

                            trace!(watched_by = %sender, watched = %receiver, "requests to watch");
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
                                let sys_msg = SysMsg::Watch(SysWatch::Down {
                                    sender:   receiver,
                                    receiver: sender,
                                    reason:   ExitReason::LinkDown,
                                });
                                let _ = subnet_mailbox_tx
                                    .tx_system
                                    .upgrade()
                                    .expect("come on! it's our own tx_system!")
                                    .send((TraceId::current(), sys_msg));
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
                                trace!(watched_by = %sender, watched = %receiver, "now watched");
                                job_entry.watched_by.insert((sender, watch_ref));
                            } else {
                                trace!(
                                    watched_by = %sender, watched = %receiver,
                                    "now watched; sending Down immediately"
                                );
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
                                let range = (sender, system::WatchRef::MIN)
                                    ..=(sender, system::WatchRef::MAX);
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

                                    if let Some((tx_priority, subnet_notify)) = subnet_mailbox_tx
                                        .tx_priority
                                        .upgrade()
                                        .zip(subnet_mailbox_tx.subnet_notify.upgrade())
                                    {
                                        let message = system::Down {
                                            peer,
                                            watch_ref,
                                            normal_exit: matches!(reason, ExitReason::Normal),
                                        };
                                        let envelope = Envelope::new(
                                            EnvelopeHeader::to_address(receiver),
                                            message,
                                        )
                                        .into_erased();
                                        let _ = tx_priority.send(MessageWithoutPermit {
                                            to:      receiver,
                                            message: envelope,
                                        });
                                        subnet_notify.notify_one();
                                    }
                                }
                            }
                        },
                    }
                    ControlFlow::Continue(())
                });
                if let ControlFlow::Break(with) = cf {
                    break with
                }
            }
        };

        trace!(reason = ?exit_reason.as_ref().map_err(|e| e.as_display_chain()), "exitting");

        let unregistered = rt_api.registry().unregister(actor_subnet);
        assert!(unregistered);

        trace!("processing the remaining sys-msgs");

        rx_system.close();
        while let Some((trace_id, sys_msg)) = rx_system.recv().await {
            trace_id.scope_sync(|| {
                trace!(?sys_msg, "sys-msg");
                match sys_msg {
                    SysMsg::Kill => (),
                    SysMsg::ForkDone(fork_lease) => {
                        let JobEntry {
                            linked_to,
                            watched_by,
                            ..
                        } = job_entries
                            .remove(&fork_lease.address)
                            .unwrap_or_else(|| panic!("unknown fork: {}", fork_lease.address));
                        for peer in linked_to {
                            let msg = sys_link_exit_message(
                                exit_reason.is_ok(),
                                fork_lease.address,
                                peer,
                            );
                            let _ = rt_api.sys_send(peer, msg);
                        }
                        for peer in watched_by.into_iter().map(|(peer, _)| peer).filter_map({
                            let mut prev = None;
                            move |this| {
                                prev.replace(this)
                                    .is_none_or(|prev| prev != this)
                                    .then_some(this)
                            }
                        }) {
                            let msg = sys_watch_down_message(
                                exit_reason.is_ok(),
                                fork_lease.address,
                                peer,
                            );
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
            });
        }

        let job_entry = job_entries
            .remove(&main_fork_address)
            .expect("main job-entry is gone somewhere");

        trace!(
            linked_count = %job_entry.linked_to.len(),
            "sending Exit to the linked actors"
        );

        for peer in job_entry.linked_to {
            let msg = sys_link_exit_message(exit_reason.is_ok(), main_fork_address, peer);
            let _ = rt_api.sys_send(peer, msg);
        }

        trace!(
            watchers_count = %job_entry.watched_by.len(),
            "sending Down to the watchers"
        );

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
            let msg = sys_watch_down_message(exit_reason.is_ok(), main_fork_address, peer);
            let _ = rt_api.sys_send(peer, msg);
        }

        trace!("done");

        Ok(exit_reason)
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
