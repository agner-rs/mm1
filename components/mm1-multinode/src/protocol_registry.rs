use std::any::TypeId;
use std::collections::HashMap;
use std::collections::hash_map::Entry::*;
use std::sync::Arc;

use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::warn;
use mm1_proto_network_management::protocols::{self, LocalTypeKey};
use mm1_proto_network_management::{self as nm};
use slotmap::SlotMap;

use crate::codec::{ErasedCodec, Protocol};

#[derive(Debug, Default)]
pub(super) struct ProtocolRegistry {
    protocols: HashMap<nm::ProtocolName, Arc<Protocol>>,

    codecs:           SlotMap<LocalTypeKey, CodecEntry>,
    codec_by_name:    HashMap<nm::MessageName, LocalTypeKey>,
    codec_by_type_id: HashMap<TypeId, LocalTypeKey>,
}

#[derive(Debug)]
struct CodecEntry {
    codec: ErasedCodec,
}

impl ProtocolRegistry {
    pub(super) fn register_protocol(
        &mut self,
        name: nm::ProtocolName,
        protocol: impl Into<Arc<Protocol>>,
    ) -> Result<(), ErrorOf<protocols::RegisterProtocolErrorKind>> {
        let protocol = protocol.into();
        let Self {
            protocols,
            codecs,
            codec_by_name,
            codec_by_type_id,
        } = self;

        let Vacant(entry) = protocols.entry(name) else {
            return Err(ErrorOf::new(
                protocols::RegisterProtocolErrorKind::DuplicateProtocolName,
                "duplicate protocol name",
            ))
        };

        for codec in protocol.outbound_types().chain(protocol.inbound_types()) {
            register_known_message(codecs, codec_by_name, codec_by_type_id, codec);
        }

        entry.insert(protocol);

        Ok(())
    }

    pub(super) fn unregister_protocol(
        &mut self,
        name: nm::ProtocolName,
    ) -> Result<(), ErrorOf<protocols::UnregisterProtocolErrorKind>> {
        let Self { protocols, .. } = self;
        let Occupied(mut protocol_entry) = protocols.entry(name.clone()) else {
            warn!(?name, "no protocol");
            return Err(ErrorOf::new(
                protocols::UnregisterProtocolErrorKind::NoProtocol,
                "no such protocol",
            ))
        };

        if Arc::get_mut(protocol_entry.get_mut()).is_none() {
            warn!(?name, "protocol is in use");
            Err(ErrorOf::new(
                protocols::UnregisterProtocolErrorKind::ProtocolInUse,
                "protocol in use",
            ))
        } else {
            protocol_entry.remove();
            Ok(())
        }
    }

    pub(super) fn protocol_by_name(&self, name: &str) -> Option<Arc<Protocol>> {
        self.protocols.get(name).cloned()
    }

    pub(super) fn local_type_key_by_tid(&self, tid: TypeId) -> Option<LocalTypeKey> {
        self.codec_by_type_id.get(&tid).copied()
    }

    pub(super) fn message_name_by_key(&self, key: LocalTypeKey) -> nm::MessageName {
        self.codecs[key].codec.name()
    }

    pub(super) fn register_message(&mut self, codec: ErasedCodec) -> LocalTypeKey {
        let Self {
            codecs,
            codec_by_name,
            codec_by_type_id,
            ..
        } = self;
        if codec.tid().is_some() {
            register_known_message(codecs, codec_by_name, codec_by_type_id, codec)
        } else {
            register_opaque_codec(codecs, codec_by_name, codec)
        }
    }
}

fn register_known_message(
    codecs: &mut SlotMap<LocalTypeKey, CodecEntry>,
    codec_by_name: &mut HashMap<nm::MessageName, LocalTypeKey>,
    codec_by_type_id: &mut HashMap<TypeId, LocalTypeKey>,
    codec: ErasedCodec,
) -> LocalTypeKey {
    let type_id = codec.tid().expect("should have a type-id");

    let name = codec.name();
    let by_type_id = match codec_by_type_id.entry(type_id) {
        Occupied(existing) => return *existing.get(),
        Vacant(by_type_id) => by_type_id,
    };

    match codec_by_name.entry(name.clone()) {
        Vacant(by_name) => {
            let codec_entry = CodecEntry { codec };
            let codec_key = codecs.insert(codec_entry);

            by_name.insert(codec_key);
            by_type_id.insert(codec_key);

            codec_key
        },
        Occupied(existing) => {
            let codec_key = *existing.get();
            codecs[codec_key].codec = codec;
            by_type_id.insert(codec_key);
            codec_key
        },
    }
}

fn register_opaque_codec(
    codecs: &mut SlotMap<LocalTypeKey, CodecEntry>,
    codec_by_name: &mut HashMap<nm::MessageName, LocalTypeKey>,
    codec: ErasedCodec,
) -> LocalTypeKey {
    assert!(codec.tid().is_none());

    match codec_by_name.entry(codec.name()) {
        Occupied(existing) => *existing.get(),
        Vacant(by_name) => {
            let codec_entry = CodecEntry { codec };
            let codec_key = codecs.insert(codec_entry);
            by_name.insert(codec_key);
            codec_key
        },
    }
}

#[cfg(test)]
mod tests {
    use mm1_proto::message;

    use super::*;
    use crate::codec::{ErasedCodecApi, Known, Opaque};

    #[message(base_path = ::mm1_proto)]
    struct Message;

    #[test]
    fn register_opaque() {
        let known = Known::<Message>::new();
        let opaque = Opaque(known.name());
        let type_id = TypeId::of::<Message>();
        let name = known.name();
        let mut state: ProtocolRegistry = Default::default();
        let opaque_key = state.register_message(ErasedCodec::from(opaque));
        assert_eq!(*state.codec_by_name.get(&name).unwrap(), opaque_key);
        assert!(!state.codec_by_type_id.contains_key(&type_id));
        assert!(state.codecs[opaque_key].codec.tid().is_none());
    }

    #[test]
    fn register_known() {
        let known = Known::<Message>::new();
        let type_id = TypeId::of::<Message>();
        let name = known.name();
        let mut state: ProtocolRegistry = Default::default();
        let known_key = state.register_message(ErasedCodec::from(known));
        assert_eq!(*state.codec_by_name.get(&name).unwrap(), known_key);
        assert_eq!(*state.codec_by_type_id.get(&type_id).unwrap(), known_key);
        assert_eq!(state.codecs[known_key].codec.tid().unwrap(), type_id);
    }

    #[test]
    fn reigster_message() {
        let known = Known::<Message>::new();
        let opaque = Opaque(known.name());

        let type_id = known.tid().unwrap();
        let name = known.name();

        let mut state: ProtocolRegistry = Default::default();

        let opaque_key = state.register_message(ErasedCodec::from(opaque));
        assert_eq!(*state.codec_by_name.get(&name).unwrap(), opaque_key);
        assert!(!state.codec_by_type_id.contains_key(&type_id));
        assert!(state.codecs[opaque_key].codec.tid().is_none());

        let known_key = state.register_message(ErasedCodec::from(known));
        assert_eq!(*state.codec_by_name.get(&name).unwrap(), known_key);
        assert_eq!(*state.codec_by_type_id.get(&type_id).unwrap(), known_key);
        assert_eq!(
            state.codecs[known_key].codec.tid(),
            Some(TypeId::of::<Message>())
        );

        assert_eq!(known_key, opaque_key);
    }
}
