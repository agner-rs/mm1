use std::convert::Infallible;
use std::fmt::{self, Write};
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Address(u64);

#[derive(Debug, thiserror::Error)]
#[error("couldn't parse address")]
pub struct AddressParseError;

impl Address {
    pub const fn from_u64(inner: u64) -> Self {
        Self(inner)
    }

    pub const fn into_u64(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for Address {
    type Error = Infallible;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

impl FromStr for Address {
    type Err = AddressParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address = address_str::from_str(s).ok_or(AddressParseError)?;
        Ok(Self(address))
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in address_str::to_chars(self.0) {
            f.write_char(c)?;
        }
        Ok(())
    }
}
impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(<D::Error as serde::de::Error>::custom)
    }
}

mod address_str {
    const CH_GAP: char = ':';
    const CH_OPEN: char = '<';
    const CH_CLOSE: char = '>';

    fn digits(mut u: u64) -> [u8; 16] {
        const MASK: u64 = 0xF000_0000_0000_0000;
        const OFFSET: u32 = u64::BITS - 4;

        let mut out = [0u8; 16];
        for d in out.iter_mut() {
            *d = ((u & MASK) >> OFFSET) as u8;
            assert!(*d <= 0xF);
            u <<= 4;
        }

        out
    }

    pub(super) fn from_str(s: &str) -> Option<u64> {
        let bytes = s.as_bytes();
        if (bytes.first().copied(), bytes.last().copied())
            != (Some(CH_OPEN as u8), Some(CH_CLOSE as u8))
        {
            return None
        }
        let bytes = &bytes[1..bytes.len() - 1];

        let mut gap_len = Some(16usize.checked_sub(bytes.len())?);

        let mut out = 0u64;

        for b in bytes.iter().copied() {
            out <<= 4;

            let c = b as char;

            if c == CH_GAP {
                let gap_len = gap_len.take()?;
                out <<= (4 * gap_len) as u32
            } else {
                let d = c.to_digit(16)?;
                out += d as u64;
            }
        }

        Some(out)
    }

    pub(super) fn to_chars(u: u64) -> impl Iterator<Item = char> {
        #[derive(Default, Clone, Copy)]
        struct Span {
            start: usize,
            len:   usize,
        }

        let digits = digits(u);

        let mut this: Span = Default::default();
        let mut best: Span = Default::default();

        for (idx, d) in digits.iter().copied().enumerate() {
            if d == 0 {
                this.len += 1
            } else {
                if this.len > best.len {
                    best = this;
                }
                this = Span {
                    start: idx + 1,
                    len:   0,
                };
            }
        }
        if this.len > best.len {
            best = this;
        }

        let left = digits
            .into_iter()
            .take(best.start)
            .map(|d| char::from_digit(d as u32, 16).expect("should be within `0..=F`"));

        let (center, resume_from) = if best.len > 1 {
            (Some(CH_GAP), best.start + best.len)
        } else {
            (None, best.start)
        };

        let right = digits
            .into_iter()
            .skip(resume_from)
            .map(|d| char::from_digit(d as u32, 16).expect("should be within `0..=F`"));

        [CH_OPEN]
            .into_iter()
            .chain(left)
            .chain(center)
            .chain(right)
            .chain([CH_CLOSE])
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn to_str(u: u64) -> String {
            to_chars(u).collect()
        }

        #[test]
        fn test_digits() {
            assert_eq!(
                [
                    0xa, 0xa, 0xa, 0xa, 0xb, 0xa, 0xb, 0xe, 0xf, 0xa, 0xc, 0xe, 0xd, 0xe, 0xa, 0xd
                ],
                digits(0xAAAA_BABE_FACE_DEAD)
            );
        }

        #[test]
        fn test_to_str() {
            assert_eq!(to_str(0xF0F0_F0F0_F0F0_F0F0), "<f0f0f0f0f0f0f0f0>");
            assert_eq!(to_str(0xFF00_FF00_FF00_FF00), "<ff:ff00ff00ff00>");
            assert_eq!(to_str(0xFF00_F000_FF00_FF00), "<ff00f:ff00ff00>");
            assert_eq!(to_str(0xFF00_0000_FF00_FF00), "<ff:ff00ff00>");
            assert_eq!(to_str(0xFF00_00FF_0000_FF00), "<ff:ff0000ff00>");
            assert_eq!(to_str(0xFF00_0000_0000_0000), "<ff:>");
        }

        #[test]
        fn test_from_str() {
            assert_eq!(from_str("<f0f0f0f0f0f0f0f0>"), Some(0xF0F0_F0F0_F0F0_F0F0));
            assert_eq!(from_str("<ff:ff00ff00ff00>"), Some(0xFF00_FF00_FF00_FF00));
            assert_eq!(from_str("<ff00f:ff00ff00>"), Some(0xFF00_F000_FF00_FF00));
            assert_eq!(from_str("<ff:ff00ff00>"), Some(0xFF00_0000_FF00_FF00));
            assert_eq!(from_str("<ff:ff0000ff00>"), Some(0xFF00_00FF_0000_FF00));
            assert_eq!(from_str("<ff:>"), Some(0xFF00_0000_0000_0000));
        }
    }
}
