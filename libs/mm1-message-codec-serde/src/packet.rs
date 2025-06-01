use mm1_address::address::Address;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Packet<K, B> {
    pub to:       Address,
    pub type_key: K,
    pub message:  B,
}
