use std::collections::HashMap;

use mm1_address::subnet::NetAddress;

mod actor_config;
mod rt_config;
mod validation;

pub(crate) use actor_config::EffectiveActorConfig;
use tokio::runtime::Runtime;
pub use validation::{Valid, ValidationError};

use crate::actor_key::ActorKey;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Mm1NodeConfig {
    #[serde(default = "defaults::default_subnets")]
    subnets: Vec<DeclareSubnet>,

    #[serde(default = "Default::default")]
    codecs: Vec<DeclareCodec>,

    #[serde(default)]
    actor: actor_config::ActorConfigNode,

    #[serde(default)]
    runtime: rt_config::RtConfigs,
}

impl Valid<Mm1NodeConfig> {
    pub(crate) fn actor_config(&self, actor_key: &ActorKey) -> impl EffectiveActorConfig + '_ {
        self.actor.select(actor_key)
    }

    pub(crate) fn build_runtimes(&self) -> std::io::Result<(Runtime, HashMap<String, Runtime>)> {
        self.runtime.build_runtimes()
    }

    pub(crate) fn local_subnet_address(&self) -> NetAddress {
        self.subnets
            .iter()
            .find_map(|s| matches!(s.kind, SubnetKind::Local).then_some(s.net_address))
            .expect("Valid<Mm1NodeConfig>")
    }

    #[cfg(feature = "multinode")]
    pub(crate) fn subnets(&self) -> impl Iterator<Item = &'_ DeclareSubnet> + use<'_> {
        self.subnets.iter()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DeclareSubnet {
    pub(crate) net_address: NetAddress,

    #[serde(flatten)]
    pub(crate) kind: SubnetKind,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub(crate) enum SubnetKind {
    Local,

    #[cfg(feature = "multinode")]
    Remote(RemoteSubnetConfig),
}

#[cfg(feature = "multinode")]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RemoteSubnetConfig {
    pub(crate) codec: String,

    #[serde(flatten)]
    opaque: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct DeclareCodec {
    pub(crate) name: String,
}

mod defaults;

#[cfg(test)]
mod test_serde;
#[cfg(test)]
mod test_validation;
