#![allow(dead_code)]

use std::sync::{Arc, Weak};

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::pool::Lease;
use mm1_address::subnet::NetAddress;
use mm1_common::log;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc};

#[derive(derive_more::Debug)]
pub(crate) struct Registry<S, M> {
    #[debug(skip)]
    networks: scc::TreeIndex<AddressRange, Node<S, M>>,
}

pub(crate) struct Node<S, M> {
    subnet_lease:    Arc<Lease>,
    inbox_size:      usize,
    inbox_semaphore: Arc<Semaphore>,
    mailbox_tx:      SubnetMailboxTx<S, M>,
}

pub(crate) fn new_mailbox<S, M>() -> (SubnetMailboxTx<S, M>, SubnetMailboxRx<S, M>) {
    let subnet_notify = Arc::new(Notify::new());
    let (tx_system, rx_system) = mpsc::unbounded_channel();
    let (tx_priority, rx_priority) = kanal::unbounded();
    let (tx_regular, rx_regular) = kanal::unbounded();

    let tx_priority = tx_priority.into();
    let tx_regular = tx_regular.into();

    let tx = SubnetMailboxTx {
        tx_system,
        subnet_notify: subnet_notify.clone(),
        tx_priority,
        tx_regular,
    };
    let rx = SubnetMailboxRx {
        rx_system,
        subnet_notify,
        rx_priority,
        rx_regular,
    };

    (tx, rx)
}

pub(crate) struct SubnetMailboxTx<S, M> {
    pub(crate) tx_system: mpsc::UnboundedSender<S>,

    pub(crate) subnet_notify: Arc<Notify>,
    pub(crate) tx_priority:   Arc<kanal::Sender<MessageWithoutPermit<M>>>,
    pub(crate) tx_regular:    Arc<kanal::Sender<MessageWithPermit<M>>>,
}

pub(crate) struct WeakSubnetMailboxTx<S, M> {
    pub(crate) tx_system: mpsc::WeakUnboundedSender<S>,

    pub(crate) subnet_notify: Weak<Notify>,
    pub(crate) tx_priority:   Weak<kanal::Sender<MessageWithoutPermit<M>>>,
    pub(crate) tx_regular:    Weak<kanal::Sender<MessageWithPermit<M>>>,
}

pub(crate) struct SubnetMailboxRx<S, M> {
    pub(crate) rx_system:     mpsc::UnboundedReceiver<S>,
    pub(crate) subnet_notify: Arc<Notify>,
    pub(crate) rx_priority:   kanal::Receiver<MessageWithoutPermit<M>>,
    pub(crate) rx_regular:    kanal::Receiver<MessageWithPermit<M>>,
}

pub(crate) struct MessageWithoutPermit<M> {
    pub(crate) to:      Address,
    pub(crate) message: M,
}

pub(crate) struct MessageWithPermit<M> {
    pub(crate) to:      Address,
    pub(crate) message: M,
    permit:             OwnedSemaphorePermit,
}

impl<S, M> Registry<S, M>
where
    S: 'static,
    M: 'static,
{
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn register(
        &self,
        subnet_address: NetAddress,
        node: Node<S, M>,
    ) -> Result<(), Node<S, M>> {
        let address_range = AddressRange::from(subnet_address);
        self.networks
            .insert(address_range, node)
            .inspect_err(|(address_range, _node)| {
                log::warn!("failed to bind address range: {}", address_range)
            })
            .map_err(|(_address_range, node)| node)?;
        log::trace!("register: registered {}", address_range);
        Ok(())
    }

    pub(crate) fn unregister(&self, subnet_address: NetAddress) -> bool {
        let guard = Default::default();
        let sought_range = AddressRange::from(subnet_address);
        let Some((found_range, _)) = self.networks.peek_entry(&sought_range, &guard) else {
            log::trace!(
                "unregister: sought-range not found [sought-range: {}]",
                sought_range
            );
            return false
        };
        if *found_range != sought_range {
            log::error!(
                "unregister: sought-range is not equal to the found range [sought: {}; found: {}]",
                sought_range,
                found_range
            );
            return false
        }
        let removed = self.networks.remove(&sought_range);
        log::trace!("unregister: range {} removed — {}", sought_range, removed);

        removed
    }

    pub(crate) fn lookup(&self, address: Address) -> Option<Node<S, M>> {
        self.networks
            .peek_with(&AddressRange::from(address), |_, node| node.clone())
    }
}

impl<S, M> Node<S, M> {
    pub(crate) fn new(
        subnet_lease: Lease,
        inbox_size: usize,
        mailbox_tx: SubnetMailboxTx<S, M>,
    ) -> Self {
        let inbox_semaphore = Arc::new(Semaphore::new(inbox_size));
        let subnet_lease = Arc::new(subnet_lease);
        Self {
            subnet_lease,
            inbox_size,
            inbox_semaphore,
            mailbox_tx,
        }
    }
}

impl<S, M> Node<S, M> {
    pub(crate) fn send(&self, to: Address, priority: bool, message: M) -> Result<(), ()> {
        let Self {
            inbox_semaphore,
            mailbox_tx,
            ..
        } = self;
        let SubnetMailboxTx {
            tx_priority,
            tx_regular,
            subnet_notify,
            ..
        } = mailbox_tx;

        let sent = if priority {
            let message_without_permit = MessageWithoutPermit { to, message };
            tx_priority
                .try_send(message_without_permit)
                .map_err(|_e| ())?
        } else {
            let Ok(permit) = inbox_semaphore.clone().try_acquire_owned() else {
                return Err(())
            };
            let message_with_permit = MessageWithPermit {
                to,
                message,
                permit,
            };
            tx_regular.try_send(message_with_permit).map_err(|_e| ())?
        };
        if sent {
            subnet_notify.notify_one();
            Ok(())
        } else {
            Err(())
        }
    }

    pub(crate) fn sys_send(&self, sys_msg: S) -> Result<(), S> {
        let Self { mailbox_tx, .. } = self;
        let SubnetMailboxTx { tx_system, .. } = mailbox_tx;
        tx_system.send(sys_msg).map_err(|e| e.0)
    }
}

impl<S, M> Clone for Node<S, M> {
    fn clone(&self) -> Self {
        let Self {
            subnet_lease,
            inbox_size,
            inbox_semaphore,
            mailbox_tx,
        } = self;
        Self {
            subnet_lease:    subnet_lease.clone(),
            inbox_size:      *inbox_size,
            inbox_semaphore: inbox_semaphore.clone(),
            mailbox_tx:      mailbox_tx.clone(),
        }
    }
}

impl<S, M> SubnetMailboxTx<S, M> {
    pub(crate) fn downgrade(&self) -> WeakSubnetMailboxTx<S, M> {
        let Self {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        } = self;
        let tx_system = tx_system.downgrade();
        let subnet_notify = Arc::downgrade(subnet_notify);
        let tx_priority = Arc::downgrade(tx_priority);
        let tx_regular = Arc::downgrade(tx_regular);

        WeakSubnetMailboxTx {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        }
    }
}
impl<S, M> WeakSubnetMailboxTx<S, M> {
    pub(crate) fn upgrade(&self) -> Option<SubnetMailboxTx<S, M>> {
        let Self {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        } = self;
        let tx_system = tx_system.upgrade()?;
        let subnet_notify = subnet_notify.upgrade()?;
        let tx_priority = tx_priority.upgrade()?;
        let tx_regular = tx_regular.upgrade()?;

        Some(SubnetMailboxTx {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        })
    }
}

impl<S, M> Clone for SubnetMailboxTx<S, M> {
    fn clone(&self) -> Self {
        let Self {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        } = self;
        Self {
            tx_system:     tx_system.clone(),
            subnet_notify: subnet_notify.clone(),
            tx_priority:   tx_priority.clone(),
            tx_regular:    tx_regular.clone(),
        }
    }
}
impl<S, M> Clone for WeakSubnetMailboxTx<S, M> {
    fn clone(&self) -> Self {
        let Self {
            tx_system,
            subnet_notify,
            tx_priority,
            tx_regular,
        } = self;
        Self {
            tx_system:     tx_system.clone(),
            subnet_notify: subnet_notify.clone(),
            tx_priority:   tx_priority.clone(),
            tx_regular:    tx_regular.clone(),
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
