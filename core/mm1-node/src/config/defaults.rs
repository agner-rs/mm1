use mm1_address::address::Address;
use mm1_address::subnet::{NetAddress, NetMask};

use crate::config::{DefLocalSubnet, LocalSubnetKind, Mm1NodeConfig};

const DEFAULT_LOCAL_SUBNET_ADDRESS_AUTO: NetAddress = NetAddress {
    address: Address::from_u64(0xFFF80000_00000000),
    mask:    NetMask::M_13,
};
const DEFAULT_LOCAL_SUBNET_ADDRESS_BIND: NetAddress = NetAddress {
    address: Address::from_u64(0xFFF00000_00000000),
    mask:    NetMask::M_13,
};

pub(super) fn local_subnets() -> Vec<DefLocalSubnet> {
    vec![
        DefLocalSubnet {
            net:  DEFAULT_LOCAL_SUBNET_ADDRESS_AUTO,
            kind: LocalSubnetKind::Auto,
        },
        DefLocalSubnet {
            net:  DEFAULT_LOCAL_SUBNET_ADDRESS_BIND,
            kind: LocalSubnetKind::Bind,
        },
    ]
}

impl Default for Mm1NodeConfig {
    fn default() -> Self {
        Self {
            local_subnets: local_subnets(),
            actor: Default::default(),
            runtime: Default::default(),
            #[cfg(feature = "multinode")]
            inbound: Default::default(),
            #[cfg(feature = "multinode")]
            outbound: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Mm1NodeConfig;

    #[test]
    fn default_mm1_node_config_is_valid() {
        Mm1NodeConfig::default()
            .validate()
            .expect("default config is not valid");
    }
}
