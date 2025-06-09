use std::any::TypeId;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use mm1_common::types::AnyError;
use mm1_core::message::AnyMessage;
use mm1_core::prim::Message;

#[derive(Debug, Default, Clone)]
pub struct CodecRegistry {
    codecs: HashMap<String, Arc<Codec>>,
}

#[derive(Debug, Default)]
pub struct Codec {
    types: HashMap<String, SupportedType>,
}

#[derive(derive_more::Debug)]
pub struct SupportedType {
    supported_type_id: TypeId,

    #[debug(skip)]
    json_codec: Box<dyn FormatSpecific<serde_json::Value> + Send + Sync + 'static>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self {
            codecs: Default::default(),
        }
    }

    pub fn get_codec(&self, name: &str) -> Option<&Codec> {
        self.codecs.get(name).map(AsRef::as_ref)
    }

    pub fn add_codec(&mut self, codec_name: &str, codec: Codec) -> &mut Self {
        self.codecs.insert(codec_name.into(), Arc::new(codec));
        self
    }
}

impl Codec {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn supported_types(&self) -> impl Iterator<Item = (TypeId, &'_ str)> + use<'_> {
        self.types
            .iter()
            .map(|(name, st)| (st.supported_type_id(), name.as_str()))
    }

    pub fn json(
        &self,
        type_name: &str,
    ) -> Option<impl FormatSpecific<serde_json::Value> + use<'_>> {
        self.types.get(type_name)
    }

    pub fn add_type<T>(&mut self) -> &mut Self
    where
        T: Message + serde::Serialize + serde::de::DeserializeOwned + 'static,
        SerdeJsonCodec<T>: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let type_name = std::any::type_name::<T>();
        let json_codec = Box::new(SerdeJsonCodec::<T>(Default::default()));

        self.types.insert(
            type_name.into(),
            SupportedType {
                supported_type_id: type_id,
                json_codec,
            },
        );
        self
    }

    pub fn with_type<T>(mut self) -> Self
    where
        T: Message + serde::Serialize + serde::de::DeserializeOwned + 'static,
        SerdeJsonCodec<T>: Send + Sync + 'static,
    {
        self.add_type::<T>();
        self
    }
}

impl SupportedType {
    pub fn supported_type_id(&self) -> TypeId {
        self.supported_type_id
    }
}

pub trait FormatSpecific<V>: Encode<V> + Decode<V> {}
impl<V, T> FormatSpecific<V> for T where T: Encode<V> + Decode<V> {}

pub trait Encode<V> {
    fn encode(&self, any_message: AnyMessage) -> Result<V, AnyError>;
}
pub trait Decode<V> {
    fn decode(&self, value: V) -> Result<AnyMessage, AnyError>;
}

impl<T, V> Encode<V> for &'_ T
where
    T: Encode<V>,
{
    fn encode(&self, any_message: AnyMessage) -> Result<V, AnyError> {
        Encode::encode(*self, any_message)
    }
}
impl<T, V> Decode<V> for &'_ T
where
    T: Decode<V>,
{
    fn decode(&self, value: V) -> Result<AnyMessage, AnyError> {
        Decode::decode(*self, value)
    }
}

pub struct SerdeJsonCodec<T>(PhantomData<T>);

impl<T> Encode<serde_json::Value> for SerdeJsonCodec<T>
where
    T: Message + serde::Serialize + 'static,
{
    fn encode(&self, any_message: AnyMessage) -> Result<serde_json::Value, AnyError> {
        let message: T = any_message.cast().map_err(|_| "unexpected message type")?;
        let encoded = serde_json::to_value(message)?;
        Ok(encoded)
    }
}

impl<T> Decode<serde_json::Value> for SerdeJsonCodec<T>
where
    T: Message + serde::de::DeserializeOwned + 'static,
{
    fn decode(&self, value: serde_json::Value) -> Result<AnyMessage, AnyError> {
        let message: T = serde_json::from_value(value)?;
        let any_message = AnyMessage::new(message);
        Ok(any_message)
    }
}

impl Decode<serde_json::Value> for SupportedType {
    fn decode(&self, value: serde_json::Value) -> Result<AnyMessage, AnyError> {
        self.json_codec.decode(value)
    }
}

impl Encode<serde_json::Value> for SupportedType {
    fn encode(&self, any_message: AnyMessage) -> Result<serde_json::Value, AnyError> {
        self.json_codec.encode(any_message)
    }
}
