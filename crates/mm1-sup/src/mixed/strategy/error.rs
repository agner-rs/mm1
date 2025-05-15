#[derive(Debug, thiserror::Error)]
pub enum DeciderError {
    #[error("duplicate key")]
    DuplicateKey,
    #[error("key not found")]
    KeyNotFound,
}
