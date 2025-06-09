use std::ops::Deref;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::address::Address;
use crate::subnet::{NetAddress, NetMask};

mod trie;

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("no available addresses in the pool")]
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct Pool {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Debug)]
pub struct Lease {
    net_address: NetAddress,
    shared:      Weak<Mutex<Shared>>,
}

#[derive(Debug)]
struct Shared {
    main: trie::Pool,
    used: trie::Pool,
}

impl Pool {
    pub fn new(net_address: NetAddress) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            main: trie::Pool::new(net_address.address.into_u64(), net_address.mask.into_u64()),
            used: trie::Pool::empty(),
        }));
        Self { shared }
    }

    pub fn lease(&self, mask: NetMask) -> Result<Lease, LeaseError> {
        let addr = {
            let mut shared = self.shared.lock();
            let Shared { main, used } = &mut *shared;

            if let Some(a) = main.acquire(mask.into_u64()) {
                a
            } else if let Some(a) = used.acquire(mask.into_u64()) {
                std::mem::swap(main, used);
                a
            } else {
                return Err(LeaseError::Unavailable);
            }
        };

        let net_address = (Address::from_u64(addr), mask).into();
        let shared = Arc::downgrade(&self.shared);
        let lease = Lease {
            net_address,
            shared,
        };
        Ok(lease)
    }
}

impl Lease {
    pub fn trusted(net_address: NetAddress) -> Self {
        Self {
            net_address,
            shared: Weak::new(),
        }
    }

    pub fn net_address(&self) -> NetAddress {
        self.net_address
    }
}

impl Deref for Lease {
    type Target = NetAddress;

    fn deref(&self) -> &Self::Target {
        &self.net_address
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            let addr = self.net_address.address.into_u64();
            let mask = self.net_address.mask.into_u64();
            shared.lock().used.release(addr, mask);
        }
    }
}
