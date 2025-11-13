use std::sync::Arc;

pub mod iface;
pub mod protocols;

pub type ProtocolName = Arc<str>;
pub type MessageName = Arc<str>;
pub type Options = serde_value::Value;
