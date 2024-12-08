use std::collections::{HashMap, HashSet};

use mm1_address::subnet::NetAddress;
use tokio::runtime::Runtime;

use crate::runtime::actor_key::ActorKey;

mod actor_config;
mod rt_config;

pub(crate) use actor_config::EffectiveActorConfig;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Mm1Config {
    #[serde(default = "defaults::subnet_address")]
    pub(crate) subnet_address: NetAddress,

    #[serde(default)]
    actor: actor_config::ActorConfigNode,

    #[serde(default)]
    runtime: rt_config::RtConfigs,
}

impl Mm1Config {
    pub(crate) fn actor_config(&self, actor_key: &ActorKey) -> impl EffectiveActorConfig + '_ {
        self.actor.select(actor_key)
    }

    pub(crate) fn build_runtimes(&self) -> std::io::Result<(Runtime, HashMap<String, Runtime>)> {
        self.runtime.build_runtimes()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let runtime_keys: HashSet<_> = self.runtime.runtime_keys().collect();
        self.actor.ensure_runtime_keys_are_valid(&runtime_keys)?;

        Ok(())
    }
}

impl Default for Mm1Config {
    fn default() -> Self {
        Self {
            subnet_address: consts::DEFAULT_SUBNET_ADDRESS,
            actor:          Default::default(),
            runtime:        Default::default(),
        }
    }
}

pub mod consts {
    use mm1_address::address::Address;
    use mm1_address::subnet::{NetAddress, NetMask};

    pub const DEFAULT_SUBNET_ADDRESS: NetAddress = NetAddress {
        address: Address::from_u64(0xFFFFFFFF_00000000),
        mask:    NetMask::M_32,
    };

    pub const DEFAULT_ACTOR_NETMASK: NetMask = NetMask::M_56;
    pub const DEFAULT_ACTOR_INBOX_SIZE: usize = 1024;
}

mod defaults {
    use mm1_address::subnet::NetAddress;

    use super::consts;
    pub(super) const fn subnet_address() -> NetAddress {
        consts::DEFAULT_SUBNET_ADDRESS
    }
}
