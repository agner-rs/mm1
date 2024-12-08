use std::any::Any;
use std::fmt;

pub use mm1_proto::Message;

pub struct AnyMessage(
    Box<dyn Any + Send + 'static>,
    #[cfg(debug_assertions)] &'static str,
);

pub struct Priority(pub AnyMessage);

impl AnyMessage {
    pub(crate) fn new<T>(value: T) -> Self
    where
        T: Message,
    {
        Self(
            Box::new(value),
            #[cfg(debug_assertions)]
            std::any::type_name::<T>(),
        )
    }

    pub(crate) fn peek<T>(&self) -> Option<&T>
    where
        T: Message,
    {
        self.0.downcast_ref()
    }

    pub(crate) fn cast<T>(self) -> Result<T, Self>
    where
        T: Message,
    {
        self.0.downcast().map(|b| *b).map_err(|value| {
            Self(
                value,
                #[cfg(debug_assertions)]
                self.1,
            )
        })
    }
}

impl fmt::Debug for AnyMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("AnyMessage");

        #[cfg(debug_assertions)]
        s.field("type_name", &self.1);

        s.field("type_id", &(*self.0).type_id()).finish()
    }
}
