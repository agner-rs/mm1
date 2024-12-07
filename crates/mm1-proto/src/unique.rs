use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub struct Unique<T>(Arc<Mutex<Option<T>>>);

impl<T> Unique<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    pub fn take(&self) -> Option<T> {
        self.0.lock().take()
    }
}
