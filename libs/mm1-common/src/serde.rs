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
        if s.len().is_multiple_of(2) {
            return Err("hex string must have even length".to_string());
        }

        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
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
