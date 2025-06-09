use mm1_address::address::Address;
use mm1_address::subnet::{NetAddress, NetMask};

pub const SUBNET_WELL_KNOWN: NetAddress = NetAddress {
    address: Address::from_u64(0xFFFF_FFFF_FFFF_0000),
    mask:    NetMask::M_48,
};

pub const NAME_SERVICE: Address = Address::from_u64(0xFFFF_FFFF_FFFF_0001);
pub const NETWORK_MANAGER: Address = Address::from_u64(0xFFFF_FFFF_FFFF_0002);

#[cfg(test)]
mod tests {
    use mm1_address::address_range::AddressRange;
    use mm1_address::subnet::NetAddress;

    use crate::SUBNET_WELL_KNOWN;

    #[test]
    fn subnet_is_valid() {
        let range = AddressRange::from(SUBNET_WELL_KNOWN);
        let subnet = NetAddress::from(range);

        assert_eq!(subnet, SUBNET_WELL_KNOWN);
    }
}
