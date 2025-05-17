use std::fmt;
use std::str::FromStr;

use crate::address::{Address, AddressParseError};

mod net_mask;

const ADDRESS_BITS: u8 = u64::BITS as u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NetMask(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetAddress {
    pub address: Address,
    pub mask:    NetMask,
}

#[derive(Debug, thiserror::Error)]
pub enum MaskParseError {
    #[error("invalid mask")]
    InvalidMask(InvalidMask),
    #[error("parse int error")]
    ParseIntError(<u64 as FromStr>::Err),
}

#[derive(Debug, thiserror::Error)]
#[error("invalid mask: {}", _0)]
pub struct InvalidMask(u8);

#[derive(Debug, thiserror::Error)]
pub enum NetAddressParseError {
    #[error("no slash")]
    NoSlash,
    #[error("parse addr error")]
    ParseAddrError(AddressParseError),
    #[error("parse mask error")]
    ParseMaskError(MaskParseError),
}

impl NetMask {
    pub fn bits_fixed(&self) -> u8 {
        self.0
    }

    pub fn bits_available(&self) -> u8 {
        ADDRESS_BITS - self.bits_fixed()
    }

    pub fn into_u64(self) -> u64 {
        match self.0 as u32 {
            0 => 0u64,
            zero_to_63 => u64::MAX << (u64::BITS - zero_to_63),
        }
    }
}

impl From<(Address, NetMask)> for NetAddress {
    fn from((address, mask): (Address, NetMask)) -> Self {
        Self { address, mask }
    }
}

impl TryFrom<u8> for NetMask {
    type Error = InvalidMask;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0..=ADDRESS_BITS => Ok(Self(value)),
            invalid => Err(InvalidMask(invalid)),
        }
    }
}

impl From<NetMask> for u8 {
    fn from(value: NetMask) -> Self {
        value.0
    }
}

impl FromStr for NetMask {
    type Err = MaskParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u8>()
            .map_err(MaskParseError::ParseIntError)?
            .try_into()
            .map_err(MaskParseError::InvalidMask)
    }
}

impl FromStr for NetAddress {
    type Err = NetAddressParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (addr, mask) = s.split_once('/').ok_or(NetAddressParseError::NoSlash)?;
        let address = addr.parse().map_err(NetAddressParseError::ParseAddrError)?;
        let mask = mask.parse().map_err(NetAddressParseError::ParseMaskError)?;

        Ok(Self { address, mask })
    }
}

impl fmt::Display for NetMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Display for NetAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.address, self.mask)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for NetAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for NetAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(<D::Error as serde::de::Error>::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mask() {
        for i in 0..=64 {
            let _: NetMask = i.to_string().parse().expect("parse mask");
        }
        for i in 65..128 {
            assert!(NetMask::from_str(&i.to_string()).is_err());
        }
    }

    #[test]
    fn net_address_parse() {
        for i in 0..=64 {
            let address = Address::from_u64(i);
            let mask = i.to_string().parse().unwrap();
            eprintln!("{}", NetAddress { address, mask });
        }
    }

    #[test]
    fn mask_to_u64() {
        for (input, expected) in [
            (64, u64::MAX),
            (60, 0xFFFF_FFFF_FFFF_FFF0),
            (56, 0xFFFF_FFFF_FFFF_FF00),
            (52, 0xFFFF_FFFF_FFFF_F000),
            (48, 0xFFFF_FFFF_FFFF_0000),
            (44, 0xFFFF_FFFF_FFF0_0000),
            (40, 0xFFFF_FFFF_FF00_0000),
            (36, 0xFFFF_FFFF_F000_0000),
            (32, 0xFFFF_FFFF_0000_0000),
            (28, 0xFFFF_FFF0_0000_0000),
            (24, 0xFFFF_FF00_0000_0000),
            (20, 0xFFFF_F000_0000_0000),
            (16, 0xFFFF_0000_0000_0000),
            (12, 0xFFF0_0000_0000_0000),
            (8, 0xFF00_0000_0000_0000),
            (4, 0xF000_0000_0000_0000),
            (0, 0x0000_0000_0000_0000),
        ] {
            assert_eq!(NetMask(input).into_u64(), expected, "/{}", input);
        }
    }
}
