use std::any::TypeId;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;

use eyre::Context;
use mm1_common::types::AnyError;
use mm1_core::message::AnyMessage;
use mm1_proto::Message;
use mm1_proto_network_management::{self as nm};
use serde::de;
use slotmap::SlotMap;

slotmap::new_key_type! {
    struct CodecKey;
}

#[derive(Default, Debug)]
pub struct Protocol {
    codecs:       SlotMap<CodecKey, Arc<dyn ErasedCodecApi>>,
    by_type_id:   HashMap<TypeId, CodecKey>,
    by_type_name: HashMap<nm::MessageName, CodecKey>,
}

pub struct Known<T> {
    message_name: nm::MessageName,
    message_type: PhantomData<T>,
}

#[derive(Debug)]
pub struct Opaque(pub nm::MessageName);

#[derive(Debug, Clone)]
pub struct ErasedCodec(Arc<dyn ErasedCodecApi>);

impl Protocol {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_type<T>(mut self) -> Self
    where
        Known<T>: ErasedCodecApi,
        T: Message,
    {
        self.add_type::<T>();
        self
    }

    pub fn add_type<T>(&mut self) -> &mut Self
    where
        Known<T>: ErasedCodecApi,
        T: Message,
    {
        let codec = Known::<T>::new();
        let erased_codec: Arc<dyn ErasedCodecApi> = Arc::new(codec);

        self.tmp_add_any_codec_really(erased_codec)
    }
}

impl Protocol {
    fn tmp_add_any_codec_really(&mut self, erased_codec: Arc<dyn ErasedCodecApi>) -> &mut Self {
        use std::collections::hash_map::Entry::*;

        let type_id_opt = erased_codec.tid();
        let type_name = erased_codec.name();

        let Vacant(by_type_name) = self.by_type_name.entry(type_name.clone()) else {
            return self
        };
        let by_type_id_opt = if let Some(type_id) = type_id_opt {
            let Vacant(by_type_id) = self.by_type_id.entry(type_id) else {
                panic!(
                    "type-name is unique, but the TypeId is not [{:?}; {}]",
                    type_id, type_name
                )
            };
            Some(by_type_id)
        } else {
            None
        };

        let key = self.codecs.insert(erased_codec);

        by_type_name.insert(key);
        if let Some(by_type_id) = by_type_id_opt {
            by_type_id.insert(key);
        }

        self
    }

    pub(crate) fn add_outbound_codec(&mut self, codec: ErasedCodec) -> Result<(), AnyError> {
        let ErasedCodec(erased_codec) = codec;
        self.tmp_add_any_codec_really(erased_codec);
        Ok(())
    }

    pub(crate) fn add_inbound_codec(&mut self, codec: ErasedCodec) -> Result<(), AnyError> {
        let ErasedCodec(erased_codec) = codec;
        self.tmp_add_any_codec_really(erased_codec);
        Ok(())
    }

    pub(crate) fn outbound_types(&self) -> impl Iterator<Item = ErasedCodec> + use<'_> {
        self.codecs.iter().map(|(_, c)| ErasedCodec(c.clone()))
    }

    pub(crate) fn inbound_types(&self) -> impl Iterator<Item = ErasedCodec> + use<'_> {
        self.codecs.iter().map(|(_, c)| ErasedCodec(c.clone()))
    }
}

impl<T> Default for Known<T> {
    fn default() -> Self {
        let message_name = std::any::type_name::<T>().into();
        Self {
            message_name,
            message_type: Default::default(),
        }
    }
}

impl<T> Known<T>
where
    T: Message,
{
    pub fn new() -> Self {
        Default::default()
    }
}

pub trait ErasedCodecApi: Debug + Send + Sync + 'static {
    fn tid(&self) -> Option<TypeId>;
    fn name(&self) -> Arc<str>;
    fn encode(&self, message: &AnyMessage, output: &mut dyn std::io::Write)
    -> Result<(), AnyError>;
    fn decode(&self, body: &[u8]) -> Result<AnyMessage, AnyError>;
}

impl<T> ErasedCodecApi for Known<T>
where
    T: Message,
    T: serde::Serialize + serde::de::DeserializeOwned,
    T: Send + Sync + 'static,
{
    fn tid(&self) -> Option<TypeId> {
        Some(TypeId::of::<T>())
    }

    fn name(&self) -> Arc<str> {
        self.message_name.clone()
    }

    fn encode(
        &self,
        message: &AnyMessage,
        output: &mut dyn std::io::Write,
    ) -> Result<(), AnyError> {
        let typed_message: &T = message
            .peek()
            .ok_or_else(|| eyre::format_err!("incompatible message type"))?;
        let () =
            rmp_serde::encode::write(output, typed_message).wrap_err("rmp_serde::encode::write")?;
        Ok(())
    }

    fn decode(&self, body: &[u8]) -> Result<AnyMessage, AnyError> {
        let typed_message: T =
            rmp_serde::decode::from_slice(body).wrap_err("rmp_serde::decode::from_slice")?;
        let any_message = AnyMessage::new(typed_message);
        Ok(any_message)
    }
}

impl ErasedCodecApi for Opaque {
    fn tid(&self) -> Option<TypeId> {
        None
    }

    fn name(&self) -> nm::MessageName {
        self.0.clone()
    }

    fn encode(
        &self,
        _message: &AnyMessage,
        _output: &mut dyn std::io::Write,
    ) -> Result<(), AnyError> {
        Err(eyre::format_err!("this is an opaque codec"))
    }

    fn decode(&self, _body: &[u8]) -> Result<AnyMessage, AnyError> {
        Err(eyre::format_err!("this is an opaque codec"))
    }
}

impl Deref for ErasedCodec {
    type Target = dyn ErasedCodecApi;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T> fmt::Debug for Known<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Known")
            .field(&std::any::type_name::<T>())
            .finish()
    }
}

impl From<Opaque> for ErasedCodec {
    fn from(opaque: Opaque) -> Self {
        Self(Arc::new(opaque))
    }
}

impl<T> From<Known<T>> for ErasedCodec
where
    T: Message + de::DeserializeOwned + Send + Sync + 'static,
{
    fn from(known: Known<T>) -> Self {
        Self(Arc::new(known))
    }
}
