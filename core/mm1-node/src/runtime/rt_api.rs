use std::collections::HashMap;
use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::pool::{Lease, Pool as SubnetPool};
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_core::context::SendErrorKind;
use mm1_core::envelope::Envelope;
use mm1_core::tracing::TraceId;
use tokio::runtime::Handle;
use tracing::trace;

use crate::registry::Registry;
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
    registry:    Registry<(TraceId, SysMsg), Envelope>,
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

    pub(crate) fn registry(&self) -> &Registry<(TraceId, SysMsg), Envelope> {
        &self.inner.registry
    }

    pub(crate) fn sys_send(&self, to: Address, sys_msg: SysMsg) -> Result<(), SendErrorKind> {
        trace!("sys_send [to: {}; sys_msg: {:?}]", to, sys_msg);

        self.inner
            .registry
            .lookup(to)
            .ok_or(SendErrorKind::NotFound)?
            .sys_send((TraceId::current(), sys_msg))
            .map_err(|_| SendErrorKind::Closed)
    }

    pub(crate) fn send_to(
        &self,
        to: Address,
        priority: bool,
        outbound: Envelope,
    ) -> Result<(), SendErrorKind> {
        self.inner
            .registry
            .lookup(to)
            .ok_or(SendErrorKind::NotFound)?
            .send(to, priority, outbound)
            .map_err(|_| SendErrorKind::Closed)
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
