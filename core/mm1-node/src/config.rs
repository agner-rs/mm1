use std::collections::HashMap;

use eyre::Context;
use mm1_address::subnet::NetAddress;
use mm1_common::types::AnyError;

mod actor_config;
mod rt_config;
mod validation;

pub(crate) use actor_config::EffectiveActorConfig;
use tokio::runtime::Runtime;
use url::Url;
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
}

#[cfg(feature = "multinode")]
pub(crate) use multinode::*;
#[cfg(feature = "multinode")]
mod multinode {

    use std::net::{IpAddr, SocketAddr};
    use std::path::Path;
    use std::{fmt, str};

    use mm1_proto_network_management as nm;
    use serde::Deserialize;

    use super::*;

    impl Valid<Mm1NodeConfig> {
        pub(crate) fn multinode_inbound(
            &self,
        ) -> impl Iterator<Item = (nm::ProtocolName, DefAddr)> + '_ {
            self.inbound
                .iter()
                .map(|d| (d.proto.clone(), d.addr.clone()))
        }

        pub(crate) fn multinode_outbound(
            &self,
        ) -> impl Iterator<Item = (nm::ProtocolName, DefAddr)> + '_ {
            self.outbound
                .iter()
                .map(|d| (d.proto.clone(), d.addr.clone()))
        }
    }

    #[derive(Debug, Clone)]
    pub(crate) enum DefAddr {
        Tcp(SocketAddr),
        Uds(Box<Path>),
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    pub(super) struct DefMultinodeInbound {
        proto: nm::ProtocolName,
        addr:  DefAddr,
    }

    #[cfg(feature = "multinode")]
    #[derive(Debug, Clone, serde::Deserialize)]
    pub(super) struct DefMultinodeOutbound {
        proto: nm::ProtocolName,
        addr:  DefAddr,
    }

    impl std::fmt::Display for DefAddr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Tcp(tcp) => write!(f, "tcp://{}", tcp),
                Self::Uds(uds) => write!(f, "uds://{:?}", uds),
            }
        }
    }

    impl std::str::FromStr for DefAddr {
        type Err = AnyError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let url: Url = s.parse().wrap_err("Url::from_str")?;
            match url.scheme() {
                "uds" => {
                    let s = format!(
                        "{}{}",
                        url.host().map(|d| d.to_string()).unwrap_or_default(),
                        url.path()
                    );
                    let p: &Path = Path::new(s.as_str());
                    Ok(Self::Uds(p.into()))
                },
                "tcp" => {
                    let ip: IpAddr = url
                        .host()
                        .ok_or(eyre::format_err!("url.host must be present"))?
                        .to_string()
                        .parse()
                        .wrap_err("IpAddr::parse")?;
                    let port: u16 = url
                        .port()
                        .ok_or(eyre::format_err!("url.port must be present"))?;
                    let socket_addr = SocketAddr::new(ip, port);
                    Ok(Self::Tcp(socket_addr))
                },
                unsupported => Err(eyre::format_err!("unsupported url-scheme: {}", unsupported)),
            }
        }
    }

    impl<'de> Deserialize<'de> for DefAddr {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            use serde::de::Error as _;
            String::deserialize(deserializer)?
                .parse()
                .map_err(D::Error::custom)
        }
    }
}

mod defaults;
