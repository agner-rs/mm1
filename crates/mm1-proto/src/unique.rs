use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug)]
pub struct Unique<T>(Arc<Mutex<Option<T>>>);

impl<T> Clone for Unique<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Unique<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    pub fn take(&self) -> Option<T> {
        self.0.lock().take()
    }
}

impl<T> serde::Serialize for Unique<T> {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(<S::Error as serde::ser::Error>::custom(
            "`Unique<_>` is not supposed to cross the node's boundary",
        ))
    }
}
impl<'de, T> serde::Deserialize<'de> for Unique<T> {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Err(<D::Error as serde::de::Error>::custom(
            "`Unique<_>` is not supposed to cross the node's boundary",
        ))
    }
}
