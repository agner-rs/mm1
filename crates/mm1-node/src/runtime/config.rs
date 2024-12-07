use mm1_address::subnet::NetAddress;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RtConfig {
    pub subnet_address: NetAddress,
}

impl Default for RtConfig {
    fn default() -> Self {
        Self {
            subnet_address: consts::DEFAULT_SUBNET_ADDRESS,
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
}

pub mod stubs {
    use mm1_address::subnet::NetMask;

    pub const ACTOR_NETMASK: NetMask = NetMask::M_56;
    pub const INBOX_SIZE: usize = 1024;
    pub const FORKED_CONTEXT_INBOX_SIZE: usize = 32;
}
