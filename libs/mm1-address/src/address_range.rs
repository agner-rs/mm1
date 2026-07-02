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
///
/// Because overlap-as-equality is not transitive, this `Ord` is a valid total
/// order only over a set of **disjoint** ranges. That is the intended use: a
/// point range (`from(Address)`) compares *equal* to the single disjoint subnet
/// that contains it, which is how the runtime looks a subnet up by address. Any
/// collection keyed by `AddressRange` must keep its keys disjoint (callers
/// check for overlap before inserting); mixing overlapping-but-distinct ranges
/// makes lookups unspecified.
#[derive(Debug, Clone, Copy)]
pub struct AddressRange {
    lo: Address,
    hi: Address,
}

impl AddressRange {
    fn new(lo: Address, hi: Address) -> Self {
        debug_assert!(lo <= hi, "AddressRange lo must not exceed hi: {lo}-{hi}");
        Self { lo, hi }
    }

    pub fn lo(&self) -> Address {
        self.lo
    }

    pub fn hi(&self) -> Address {
        self.hi
    }
}

impl From<Address> for AddressRange {
    fn from(addr: Address) -> Self {
        Self::new(addr, addr)
    }
}
impl From<NetAddress> for AddressRange {
    fn from(net: NetAddress) -> Self {
        let lo = net.address;
        let hi = Address::from_u64(net.address.into_u64() | !net.mask.into_u64());

        Self::new(lo, hi)
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

        // `lo <= hi` is guaranteed at construction, so it is not re-checked on
        // this hot comparison path.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn range(lo: u64, hi: u64) -> AddressRange {
        AddressRange::new(Address::from_u64(lo), Address::from_u64(hi))
    }

    // Locks the intended semantics for #148: overlap-as-equality is used for
    // containment lookup, and disjoint ranges form a proper total order.
    #[test]
    fn ordering_semantics() {
        use std::cmp::Ordering::*;

        // A point inside a range compares equal to that range (containment).
        assert_eq!(range(5, 5).cmp(&range(0, 10)), Equal);
        assert_eq!(range(0, 10).cmp(&range(5, 5)), Equal);

        // Disjoint ranges order by position.
        assert_eq!(range(0, 10).cmp(&range(11, 20)), Less);
        assert_eq!(range(11, 20).cmp(&range(0, 10)), Greater);

        // Touching boundaries overlap, hence compare equal.
        assert_eq!(range(0, 10).cmp(&range(10, 20)), Equal);
    }
}
