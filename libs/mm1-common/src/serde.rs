pub mod no_serde {
    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S, T>(_: T, _: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reason = S::Error::custom("not supported");
        Err(reason)
    }
    pub fn deserialize<'de, D, T>(_: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
    {
        let reason = D::Error::custom("not supported");
        Err(reason)
    }
}

pub mod binary {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    fn from_hex(s: &str) -> Result<Vec<u8>, String> {
        if !s.len().is_multiple_of(2) {
            return Err("hex string must have even length".to_string());
        }
        if !s.is_ascii() {
            return Err("hex string must be ASCII".to_string());
        }

        let bytes = s.as_bytes();
        (0..bytes.len())
            .step_by(2)
            .map(|i| {
                // Safe: `s` is ASCII and even-length, so each 2-byte window is a
                // valid `str` on char boundaries.
                let pair = std::str::from_utf8(&bytes[i..i + 2]).map_err(|e| e.to_string())?;
                u8::from_str_radix(pair, 16).map_err(|e| e.to_string())
            })
            .collect()
    }

    pub fn serialize<S, T>(bin: T, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]>,
    {
        if s.is_human_readable() {
            let hex_string = to_hex(bin.as_ref());
            s.serialize_str(&hex_string)
        } else {
            s.serialize_bytes(bin.as_ref())
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: From<Vec<u8>>,
    {
        if d.is_human_readable() {
            let s = String::deserialize(d)?;
            let bytes = from_hex(&s).map_err(D::Error::custom)?;
            Ok(T::from(bytes))
        } else {
            let bytes = Vec::<u8>::deserialize(d)?;
            Ok(T::from(bytes))
        }
    }
}

#[cfg(test)]
mod tests {
    // Regression tests for #127: the human-readable `binary` codec must
    // round-trip even-length hex and must reject bad input without panicking.
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Wrap {
        #[serde(with = "super::binary")]
        data: Vec<u8>,
    }

    #[test]
    fn binary_hex_round_trips() {
        let value = Wrap {
            data: vec![0xde, 0xad, 0xbe, 0xef],
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#"{"data":"deadbeef"}"#);
        let back: Wrap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn binary_hex_rejects_odd_length() {
        assert!(serde_json::from_str::<Wrap>(r#"{"data":"abc"}"#).is_err());
    }

    #[test]
    fn binary_hex_rejects_non_ascii() {
        assert!(serde_json::from_str::<Wrap>(r#"{"data":"a€"}"#).is_err());
    }
}
