use std::any::{Any, TypeId};
use std::fmt;

pub use mm1_proto::Message;

pub struct AnyMessage {
    message: Box<dyn Any + Send + 'static>,
    info:    TypeInfo,
}

struct TypeInfo {
    id:   TypeId,
    name: &'static str,
}

impl AnyMessage {
    pub(crate) fn new<T>(value: T) -> Self
    where
        T: Message,
    {
        let info = TypeInfo {
            id:   std::any::TypeId::of::<T>(),
            name: std::any::type_name::<T>(),
        };
        let message = Box::new(value);
        Self { info, message }
    }

    pub(crate) fn peek<T>(&self) -> Option<&T>
    where
        T: Message,
    {
        self.message.downcast_ref()
    }

    pub(crate) fn cast<T>(self) -> Result<T, Self>
    where
        T: Message,
    {
        let Self { message, info } = self;
        message
            .downcast()
            .map(|b| *b)
            .map_err(move |message| Self { info, message })
    }

    pub(crate) fn tid(&self) -> TypeId {
        self.info.id
    }

    pub(crate) fn type_name(&self) -> &'static str {
        self.info.name
    }
}

impl fmt::Debug for AnyMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyMessage")
            .field("type_name", &self.type_name())
            .field("type_id", &self.tid())
            .finish()
    }
}
