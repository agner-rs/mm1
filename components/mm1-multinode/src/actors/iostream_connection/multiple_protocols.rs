use std::sync::Arc;

use bimap::{BiMap, Overwritten};
use eyre::Context;
use mm1_common::types::AnyError;
use mm1_proto_network_management::protocols::{self as p, LocalTypeKey};

use crate::codec::Protocol;

pub(super) fn reduce(
    protocols: &[p::ProtocolResolved<Protocol>],
) -> Result<p::ProtocolResolved<Protocol>, AnyError> {
    let mut combined_protocol = Protocol::new();
    let mut combined_outbound: BiMap<Arc<str>, LocalTypeKey> = Default::default();
    let mut combined_inbound: BiMap<Arc<str>, LocalTypeKey> = Default::default();

    for p::ProtocolResolved {
        protocol,
        outbound,
        inbound,
    } in protocols
    {
        for codec in protocol.outbound_types() {
            combined_protocol
                .add_outbound_codec(codec)
                .wrap_err("combined_protocol.add_outbound_codec")?;
        }
        for codec in protocol.inbound_types() {
            combined_protocol
                .add_inbound_codec(codec)
                .wrap_err("combined_protocol.add_inbound_codec")?;
        }
        for (name, key) in outbound {
            use Overwritten::*;
            match combined_outbound.insert(name.clone(), *key) {
                Neither => (),
                Both(..) => (),
                _ => return Err(eyre::format_err!("inconsistent name-key mappings")),
            }
        }
        for (name, key) in inbound {
            use Overwritten::*;
            match combined_inbound.insert(name.clone(), *key) {
                Neither => (),
                Both(..) => (),
                _ => return Err(eyre::format_err!("inconsistent name-key mappings")),
            }
        }
    }

    Ok(p::ProtocolResolved {
        protocol: Arc::new(combined_protocol),
        outbound: combined_outbound.into_iter().collect(),
        inbound:  combined_inbound.into_iter().collect(),
    })
}
