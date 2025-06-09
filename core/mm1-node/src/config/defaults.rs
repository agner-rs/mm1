use mm1_address::address::Address;
use mm1_address::subnet::{NetAddress, NetMask};

use crate::config::{DeclareSubnet, Mm1NodeConfig, SubnetKind};

const DEFAULT_LOCAL_SUBNET_ADDRESS: NetAddress = NetAddress {
    address: Address::from_u64(0xFFFF0000_00000000),
    mask:    NetMask::M_16,
};

impl Default for Mm1NodeConfig {
    fn default() -> Self {
        Self {
            subnets: default_subnets(),
            codecs:  Default::default(),
            actor:   Default::default(),
            runtime: Default::default(),
        }
    }
}

pub(super) fn default_subnets() -> Vec<DeclareSubnet> {
    vec![DeclareSubnet {
        net_address: DEFAULT_LOCAL_SUBNET_ADDRESS,
        kind:        SubnetKind::Local,
    }]
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
