use std::fmt;

pub use tracing::Level;

#[derive(Debug, Clone, structopt::StructOpt, serde::Serialize, serde::Deserialize)]
pub struct LoggingConfig {
    #[structopt(long, default_value = "info")]
    #[serde(with = "impl_serde_for_level")]
    pub min_log_level: tracing::Level,

    #[structopt(long)]
    #[serde(default)]
    pub log_target_filter: Vec<LogTargetConfig>,
}

// TODO: make it actually work, maybe?
#[derive(Debug, Clone)]
pub struct LogTargetConfig {
    pub path:  Vec<String>,
    pub level: tracing::Level,
}

impl std::str::FromStr for LogTargetConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (path, level) = s
            .split_once('=')
            .ok_or_else(|| "eq-sign missing".to_owned())?;
        let level = level.parse::<tracing::Level>().map_err(|e| e.to_string())?;
        let path = path.split("::").map(|s| s.to_owned()).collect();

        let out = Self { path, level };
        Ok(out)
    }
}

impl fmt::Display for LogTargetConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        format!("{}={}", self.path.join("*"), self.level).fmt(f)
    }
}

mod impl_serde_for_log_target_config {
    use super::*;
    impl<'de> serde::Deserialize<'de> for LogTargetConfig {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            use serde::de::Error as DeError;
            String::deserialize(deserializer)?
                .parse()
                .map_err(D::Error::custom)
        }
    }

    impl serde::Serialize for LogTargetConfig {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.to_string().serialize(serializer)
        }
    }
}

mod impl_serde_for_level {
    use serde::de::Error as DeError;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(value: &tracing::Level, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.to_string().serialize(ser)
    }

    pub(super) fn deserialize<'de, D>(deser: D) -> Result<tracing::Level, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deser)?
            .parse::<tracing::Level>()
            .map_err(D::Error::custom)
    }
}
