#![allow(dead_code)]

use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::{Lease, LeaseError, Pool};
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_common::log;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

#[derive(derive_more::Debug)]
pub(crate) struct Registry<S, M> {
    #[debug(skip)]
    networks: scc::TreeIndex<AddressRange, Node<S, M>>,
}

pub(crate) struct ActorNode<S, M> {
    subnet_lease:         Lease,
    subnet_pool:          Pool,
    sem_regular_messages: Arc<Semaphore>,
    forks:                scc::HashMap<Address, ForkEntry<M>>,

    pub tx_system: mpsc::UnboundedSender<S>,
}

pub(crate) struct NetworkNode<S, M> {
    subnet_lease:         Lease,
    sem_regular_messages: Arc<Semaphore>,
    tx_system:            mpsc::UnboundedSender<S>,
    tx_priority:          mpsc::UnboundedSender<M>,
    tx_regular:           mpsc::UnboundedSender<MessageWithPermit<M>>,
}

pub(crate) struct MessageWithPermit<M> {
    pub(crate) message: M,
    permit:             OwnedSemaphorePermit,
}

#[derive(derive_more::From)]
pub(crate) enum Node<S, M> {
    Actor(Arc<ActorNode<S, M>>),
    Network(Arc<NetworkNode<S, M>>),
}

pub(crate) struct ForkEntry<M> {
    fork_lease:  Lease,
    tx_priority: mpsc::UnboundedSender<M>,
    tx_regular:  mpsc::UnboundedSender<MessageWithPermit<M>>,
}

impl<S, M> Registry<S, M>
where
    S: 'static,
    M: 'static,
{
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn register(&self, subnet_address: NetAddress, node: impl Into<Node<S, M>>) -> bool {
        let address_range = AddressRange::from(subnet_address);
        self.networks
            .insert(address_range, node.into())
            .inspect_err(|(address_range, _)| {
                log::warn!("failed to bind address range: {}", address_range)
            })
            .is_ok()
    }

    pub(crate) fn unregister(&self, subnet_address: NetAddress) -> bool {
        let guard = Default::default();
        let sought_range = AddressRange::from(subnet_address);
        let Some((found_range, _)) = self.networks.peek_entry(&sought_range, &guard) else {
            return false
        };
        if *found_range != sought_range {
            return false
        }
        self.networks.remove(&sought_range)
    }

    pub(crate) fn lookup(&self, address: Address) -> Option<Node<S, M>> {
        self.networks
            .peek_with(&AddressRange::from(address), |_, node| node.clone())
    }
}

impl<S, M> ActorNode<S, M> {
    pub(crate) fn new(
        subnet_lease: Lease,
        inbox_size: usize,
        tx_system: mpsc::UnboundedSender<S>,
    ) -> Self {
        let subnet_pool = Pool::new(subnet_lease.net_address());
        let sem_regular_messages = Arc::new(Semaphore::new(inbox_size));
        Self {
            subnet_lease,
            subnet_pool,
            sem_regular_messages,
            tx_system,
            forks: Default::default(),
        }
    }

    pub(crate) fn lease_address(&self) -> Result<Lease, LeaseError> {
        self.subnet_pool.lease(NetMask::M_64)
    }

    pub(crate) fn register(&self, address: Address, fork_entry: ForkEntry<M>) -> bool {
        self.forks.insert(address, fork_entry).is_ok()
    }

    pub(crate) fn unregister(&self, address: Address) -> Option<Lease> {
        let (_, ForkEntry { fork_lease, .. }) = self.forks.remove(&address)?;
        Some(fork_lease)
    }
}

impl<M> ForkEntry<M> {
    pub(crate) fn new(
        fork_lease: Lease,
        tx_priority: mpsc::UnboundedSender<M>,
        tx_regular: mpsc::UnboundedSender<MessageWithPermit<M>>,
    ) -> Self {
        assert_eq!(fork_lease.net_address().mask, NetMask::M_64);
        Self {
            fork_lease,
            tx_priority,
            tx_regular,
        }
    }
}

impl<S, M> NetworkNode<S, M> {
    pub(crate) fn new(
        subnet_lease: Lease,
        inbox_size: usize,
        tx_system: mpsc::UnboundedSender<S>,
        tx_priority: mpsc::UnboundedSender<M>,
        tx_regular: mpsc::UnboundedSender<MessageWithPermit<M>>,
    ) -> Self {
        let sem_regular_messages = Arc::new(Semaphore::new(inbox_size));
        Self {
            subnet_lease,
            sem_regular_messages,
            tx_system,
            tx_priority,
            tx_regular,
        }
    }
}

impl<S, M> Node<S, M> {
    pub(crate) fn send(&self, to: Address, priority: bool, message: M) -> Result<(), M> {
        let (sem, chans) = match self {
            Self::Actor(actor) => {
                (
                    actor.sem_regular_messages.clone(),
                    actor
                        .forks
                        .get(&to)
                        .map(|f| (f.tx_priority.clone(), f.tx_regular.clone())),
                )
            },
            Self::Network(network) => {
                (
                    network.sem_regular_messages.clone(),
                    Some((network.tx_priority.clone(), network.tx_regular.clone())),
                )
            },
        };
        let Some((tx_priority, tx_regular)) = chans else {
            return Err(message)
        };
        if priority {
            tx_priority.send(message).map_err(|e| e.0)?;
        } else {
            let Ok(permit) = sem.try_acquire_owned() else {
                return Err(message)
            };
            let message_with_permit = MessageWithPermit { message, permit };
            tx_regular
                .send(message_with_permit)
                .map_err(|e| e.0.message)?;
        }
        Ok(())
    }

    pub(crate) fn sys_send(&self, sys_msg: S) -> Result<(), S> {
        let tx_system = match self {
            Self::Actor(a) => &a.tx_system,
            Self::Network(n) => &n.tx_system,
        };
        tx_system.send(sys_msg).map_err(|e| e.0)
    }
}

impl<S, M> Clone for Node<S, M> {
    fn clone(&self) -> Self {
        match self {
            Self::Actor(a) => Self::Actor(a.clone()),
            Self::Network(n) => Self::Network(n.clone()),
        }
    }
}

impl<S, M> Default for Registry<S, M> {
    fn default() -> Self {
        Self {
            networks: Default::default(),
        }
    }
}
