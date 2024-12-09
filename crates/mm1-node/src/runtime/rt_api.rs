use std::collections::HashMap;
use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::pool::{Lease, Pool as SubnetPool};
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_core::context::TellErrorKind;
use mm1_core::envelope::Envelope;
use mq::TrySendError;
use tokio::runtime::Handle;
use tracing::trace;

use crate::runtime::mq;
use crate::runtime::registry::{
    Registry, {self},
};
use crate::runtime::sys_msg::SysMsg;

#[derive(Debug, Clone)]
pub(crate) struct RtApi {
    inner: Arc<Inner>,
}

#[derive(Debug, thiserror::Error)]
#[error("lease error: {}", _0)]
pub(crate) struct RequestAddressError(#[source] mm1_address::pool::LeaseError);

#[derive(Debug)]
struct Inner {
    subnet_pool: SubnetPool,
    registry:    Registry,
    default_rt:  Handle,
    named_rts:   HashMap<String, Handle>,
}

impl RtApi {
    pub(crate) fn create(
        subnet_address: NetAddress,
        default_rt: Handle,
        named_rts: HashMap<String, Handle>,
    ) -> Self {
        let subnet_pool = SubnetPool::new(subnet_address);
        let registry = Registry::new();
        let inner = Arc::new(Inner {
            subnet_pool,
            registry,
            default_rt,
            named_rts,
        });
        Self { inner }
    }

    pub(crate) fn register(
        &self,
        address_lease: Lease,
        tx_system: mq::UbTx<SysMsg>,
        tx_priority: mq::UbTx<Envelope>,
        tx_regular: mq::Tx<Envelope>,
    ) {
        trace!("register [address: {}]", address_lease.address);

        use registry::*;
        self.inner
            .registry
            .insert(
                address_lease.address,
                Entry {
                    address_lease,
                    // state: State::Running(Running),
                    tx_system,
                    tx_priority,
                    tx_regular,
                },
            )
            .expect(/* FIXME */ "address reused");
    }

    pub(crate) fn unregister(&self, address: Address) -> Option<(Lease, mq::UbTx<SysMsg>)> {
        trace!("unregister [addr: {}]", address);

        use registry::*;
        self.inner.registry.remove(&address).map(
            |(
                _,
                Entry {
                    address_lease,
                    tx_system,
                    ..
                },
            )| (address_lease, tx_system),
        )
    }

    pub(crate) fn sys_send(&self, to: Address, sys_msg: SysMsg) -> Result<(), TellErrorKind> {
        trace!("sys_send [to: {}; sys_msg: {:?}]", to, sys_msg);

        let entry = self
            .inner
            .registry
            .get(&to)
            .ok_or(TellErrorKind::NotFound)?;
        let tx_system = &entry.get().tx_system;
        let _ = tx_system
            .send(sys_msg)
            .map_err(|_e| TellErrorKind::Closed)?;
        Ok(())
    }

    pub(crate) fn send(
        &self,
        to: Address,
        priority: bool,
        inbound: Envelope,
    ) -> Result<(), TellErrorKind> {
        let entry = self
            .inner
            .registry
            .get(&to)
            .ok_or(TellErrorKind::NotFound)?;
        if priority {
            entry
                .get()
                .tx_priority
                .send(inbound)
                .map_err(|_| TellErrorKind::Closed)
                .map(|_| ())
        } else {
            entry
                .get()
                .tx_regular
                .try_send(inbound)
                .map_err(|e| {
                    match e {
                        TrySendError::Closed(_) => TellErrorKind::Closed,
                        TrySendError::Full(_) => TellErrorKind::Full,
                    }
                })
                .map(|_| ())
        }
    }

    pub(crate) async fn request_address(
        &self,
        mask: NetMask,
    ) -> Result<Lease, RequestAddressError> {
        self.inner
            .subnet_pool
            .lease(mask)
            .map_err(RequestAddressError)
    }

    pub(crate) fn choose_executor(&self, key: Option<&str>) -> &tokio::runtime::Handle {
        if let Some(key) = key {
            self.inner
                .named_rts
                .get(key)
                .expect("config should have been validated, shouldn't it?")
        } else {
            &self.inner.default_rt
        }
    }
}
