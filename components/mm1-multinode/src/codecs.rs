use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use mm1_common::types::AnyError;
use mm1_core::message::AnyMessage;
use mm1_proto::Message;

use crate::remote_subnet::config::SerdeFormat;

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

    #[cfg(feature = "format-json")]
    #[debug(skip)]
    format_json: Box<dyn FormatSpecific + Send + Sync + 'static>,

    #[cfg(feature = "format-bincode")]
    #[debug(skip)]
    format_bincode: Box<dyn FormatSpecific + Send + Sync + 'static>,

    #[cfg(feature = "format-rmp")]
    #[debug(skip)]
    format_rmp: Box<dyn FormatSpecific + Send + Sync + 'static>,
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

    pub(crate) fn select_type(&self, type_name: &str) -> Option<&SupportedType> {
        self.types.get(type_name)
    }

    pub fn add_type<T>(&mut self) -> &mut Self
    where
        T: Message + serde::Serialize + serde::de::DeserializeOwned + 'static,
        T: Send + Sync + 'static,
    {
        let type_name = std::any::type_name::<T>();

        self.types
            .insert(type_name.into(), SupportedType::for_type::<T>());
        self
    }

    pub fn with_type<T>(mut self) -> Self
    where
        T: Message + serde::Serialize + serde::de::DeserializeOwned + 'static,
        T: Send + Sync + 'static,
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

pub trait FormatSpecific:
    FormatSpecificEncode + FormatSpecificDecode + Send + Sync + 'static
{
}
impl<T> FormatSpecific for T where
    T: FormatSpecificEncode + FormatSpecificDecode + Send + Sync + 'static
{
}

pub trait FormatSpecificEncode {
    fn encode(&self, any_message: AnyMessage) -> Result<Bytes, AnyError>;
}
pub trait FormatSpecificDecode {
    fn decode(&self, bytes: Bytes) -> Result<AnyMessage, AnyError>;
}

#[cfg(feature = "format-json")]
pub struct SerdeJsonCodec<T>(std::marker::PhantomData<T>);

#[cfg(feature = "format-bincode")]
pub struct SerdeBincodeCodec<T>(std::marker::PhantomData<T>);

#[cfg(feature = "format-bincode")]
pub struct SerdeRmpCodec<T>(std::marker::PhantomData<T>);

impl SupportedType {
    pub(crate) fn for_type<T>() -> Self
    where
        T: Message + serde::Serialize + serde::de::DeserializeOwned + 'static,
        T: Send + Sync + 'static,
    {
        let supported_type_id = TypeId::of::<T>();

        Self {
            supported_type_id,
            #[cfg(feature = "format-json")]
            format_json: Box::new(SerdeJsonCodec::<T>(Default::default())),
            #[cfg(feature = "format-bincode")]
            format_bincode: Box::new(SerdeBincodeCodec::<T>(Default::default())),
            #[cfg(feature = "format-rmp")]
            format_rmp: Box::new(SerdeRmpCodec::<T>(Default::default())),
        }
    }

    pub(crate) fn select_format(&self, serde_format: SerdeFormat) -> &dyn FormatSpecific {
        match serde_format {
            #[cfg(feature = "format-json")]
            SerdeFormat::Json => self.format_json.as_ref(),

            #[cfg(feature = "format-bincode")]
            SerdeFormat::Bincode => self.format_bincode.as_ref(),

            #[cfg(feature = "format-rmp")]
            SerdeFormat::Rmp => self.format_rmp.as_ref(),
        }
    }
}

#[cfg(feature = "format-json")]
impl<T> FormatSpecificEncode for SerdeJsonCodec<T>
where
    T: Message + serde::Serialize + 'static,
{
    fn encode(&self, any_message: AnyMessage) -> Result<Bytes, AnyError> {
        let message: T = any_message.cast().map_err(|_| "unexpected message type")?;
        let encoded = serde_json::to_vec(&message)?;
        Ok(encoded.into())
    }
}

#[cfg(feature = "format-json")]
impl<T> FormatSpecificDecode for SerdeJsonCodec<T>
where
    T: Message + serde::de::DeserializeOwned + 'static,
{
    fn decode(&self, bytes: Bytes) -> Result<AnyMessage, AnyError> {
        let message: T = serde_json::from_slice(&bytes)?;
        let any_message = AnyMessage::new(message);
        Ok(any_message)
    }
}

#[cfg(feature = "format-bincode")]
impl<T> FormatSpecificEncode for SerdeBincodeCodec<T>
where
    T: Message + serde::Serialize + 'static,
{
    fn encode(&self, any_message: AnyMessage) -> Result<Bytes, AnyError> {
        let message: T = any_message.cast().map_err(|_| "unexpected message type")?;
        let encoded = serde_json::to_vec(&message)?;
        Ok(encoded.into())
    }
}

#[cfg(feature = "format-bincode")]
impl<T> FormatSpecificDecode for SerdeBincodeCodec<T>
where
    T: Message + serde::de::DeserializeOwned + 'static,
{
    fn decode(&self, bytes: Bytes) -> Result<AnyMessage, AnyError> {
        let message: T = serde_json::from_slice(&bytes)?;
        let any_message = AnyMessage::new(message);
        Ok(any_message)
    }
}

#[cfg(feature = "format-rmp")]
impl<T> FormatSpecificEncode for SerdeRmpCodec<T>
where
    T: Message + serde::Serialize + 'static,
{
    fn encode(&self, any_message: AnyMessage) -> Result<Bytes, AnyError> {
        let message: T = any_message.cast().map_err(|_| "unexpected message type")?;
        let encoded = rmp_serde::encode::to_vec(&message)?;
        Ok(encoded.into())
    }
}

#[cfg(feature = "format-rmp")]
impl<T> FormatSpecificDecode for SerdeRmpCodec<T>
where
    T: Message + serde::de::DeserializeOwned + 'static,
{
    fn decode(&self, bytes: Bytes) -> Result<AnyMessage, AnyError> {
        let message: T = rmp_serde::from_slice(&bytes)?;
        let any_message = AnyMessage::new(message);
        Ok(any_message)
    }
}
