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
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Link {
    pub(crate) bind: SocketAddr,
    pub(crate) peer: SocketAddr,
}
