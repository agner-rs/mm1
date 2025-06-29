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
