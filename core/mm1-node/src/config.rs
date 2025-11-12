use std::collections::HashMap;
#[cfg(feature = "multinode")]
use std::net::SocketAddr;

use mm1_address::subnet::NetAddress;
#[cfg(feature = "multinode")]
use mm1_proto_network_management as nm;

mod actor_config;
mod rt_config;
mod validation;

pub(crate) use actor_config::EffectiveActorConfig;
use tokio::runtime::Runtime;
pub use validation::{Valid, ValidationError};

use crate::actor_key::ActorKey;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Mm1NodeConfig {
    #[serde(default = "defaults::local_subnets")]
    local_subnets: Vec<DefLocalSubnet>,

    #[cfg(feature = "multinode")]
    #[serde(default = "Default::default")]
    inbound: Vec<DefMultinodeInbound>,

    #[cfg(feature = "multinode")]
    #[serde(default = "Default::default")]
    outbound: Vec<DefMultinodeOutbound>,

    #[serde(default)]
    actor: actor_config::ActorConfigNode,

    #[serde(default)]
    runtime: rt_config::RtConfigs,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DefLocalSubnet {
    net:  NetAddress,
    kind: LocalSubnetKind,
}

#[cfg(feature = "multinode")]
#[derive(Debug, Clone, serde::Deserialize)]
struct DefMultinodeInbound {
    proto:     nm::ProtocolName,
    bind_addr: SocketAddr,
}

#[cfg(feature = "multinode")]
#[derive(Debug, Clone, serde::Deserialize)]
struct DefMultinodeOutbound {
    proto:    nm::ProtocolName,
    dst_addr: SocketAddr,
    src_addr: Option<SocketAddr>,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum LocalSubnetKind {
    Auto,
    Bind,
}

impl Valid<Mm1NodeConfig> {
    pub(crate) fn actor_config(&self, actor_key: &ActorKey) -> impl EffectiveActorConfig + '_ {
        self.actor.select(actor_key)
    }

    pub(crate) fn build_runtimes(&self) -> std::io::Result<(Runtime, HashMap<String, Runtime>)> {
        self.runtime.build_runtimes()
    }

    pub(crate) fn local_subnet_address_auto(&self) -> NetAddress {
        self.local_subnets
            .iter()
            .find(|d| matches!(d.kind, LocalSubnetKind::Auto))
            .expect("exactly one local subnet must be present")
            .net
    }

    pub(crate) fn local_subnet_addresses_bind(&self) -> impl Iterator<Item = NetAddress> + '_ {
        self.local_subnets
            .iter()
            .filter_map(|d| matches!(d.kind, LocalSubnetKind::Bind).then_some(d.net))
    }

    #[cfg(feature = "multinode")]
    pub(crate) fn multinode_inbound(
        &self,
    ) -> impl Iterator<Item = (nm::ProtocolName, SocketAddr)> + '_ {
        self.inbound.iter().map(|d| (d.proto.clone(), d.bind_addr))
    }

    #[cfg(feature = "multinode")]
    pub(crate) fn multinode_outbound(
        &self,
    ) -> impl Iterator<Item = (nm::ProtocolName, SocketAddr, Option<SocketAddr>)> + '_ {
        self.outbound
            .iter()
            .map(|d| (d.proto.clone(), d.dst_addr, d.src_addr))
    }
}

mod defaults;
