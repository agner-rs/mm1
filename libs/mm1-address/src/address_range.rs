use std::fmt;

use crate::address::Address;
use crate::subnet::{NetAddress, NetMask};

/// A range of addresses corresponding to some [`NetAddress`].
///
/// The only way to create an instance of [`AddressRange`] is from a
/// [`NetAddress`] or [`Address`], thus only valid ranges are represented by
/// this type.
///
/// This type can be used as a key in a key-value collection that uses `Ord` for
/// its keys. (**NB:** the equality is defined as "if the ranges overlap — they
/// are equal")
#[derive(Debug, Clone, Copy)]
pub struct AddressRange {
    lo: Address,
    hi: Address,
}

impl AddressRange {
    pub fn lo(&self) -> Address {
        self.lo
    }

    pub fn hi(&self) -> Address {
        self.hi
    }
}

impl From<Address> for AddressRange {
    fn from(addr: Address) -> Self {
        Self { lo: addr, hi: addr }
    }
}
impl From<NetAddress> for AddressRange {
    fn from(net: NetAddress) -> Self {
        let lo = net.address;
        let hi = Address::from_u64(net.address.into_u64() | !net.mask.into_u64());

        Self { lo, hi }
    }
}

impl From<AddressRange> for NetAddress {
    fn from(range: AddressRange) -> Self {
        let AddressRange { lo, hi } = range;
        let address = lo;
        let mask_bits = !(lo.into_u64() ^ hi.into_u64());
        let mask = NetMask::try_from(mask_bits.leading_ones() as u8).expect("should be fine");
        Self { address, mask }
    }
}

impl Ord for AddressRange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;

        assert!(self.lo <= self.hi);
        assert!(other.lo <= other.hi);

        match (self.hi.cmp(&other.lo), self.lo.cmp(&other.hi)) {
            (Less, Less) => Less,
            (Greater, Greater) => Greater,
            (..) => Equal,
        }
    }
}
impl PartialEq for AddressRange {
    fn eq(&self, other: &Self) -> bool {
        Ord::cmp(self, other).is_eq()
    }
}
impl Eq for AddressRange {}
impl PartialOrd for AddressRange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl fmt::Display for AddressRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.lo, self.hi)
    }
}
