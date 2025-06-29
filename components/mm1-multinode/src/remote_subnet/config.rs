use std::net::SocketAddr;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "protocol")]
pub enum RemoteSubnetConfig {
    Wip(ProtocolWip),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ProtocolWip {
    pub(crate) codec: String,
    pub(crate) link:  Link,
    pub(crate) serde: SerdeFormat,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Link {
    pub(crate) bind: SocketAddr,
    pub(crate) peer: SocketAddr,
}

#[derive(
    Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, PartialOrd, Eq, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SerdeFormat {
    #[cfg(feature = "format-json")]
    Json,
    #[cfg(feature = "format-bincode")]
    Bincode,
    #[cfg(feature = "format-rmp")]
    Rmp,
}
