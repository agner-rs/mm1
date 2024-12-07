use std::fmt;
use std::sync::Arc;

use crate::errors::error_kind::HasErrorKind;

#[derive(Debug, thiserror::Error)]
#[error("{}: {}", kind, message)]
pub struct ErrorOf<Kind: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static> {
    pub kind:    Kind,
    pub message: Arc<str>,
}

impl<K0> ErrorOf<K0>
where
    K0: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static,
{
    pub fn map_kind<K1>(self, map: impl FnOnce(K0) -> K1) -> ErrorOf<K1>
    where
        K1: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static,
    {
        let ErrorOf { kind, message } = self;
        let kind = map(kind);
        ErrorOf { kind, message }
    }
}

impl<K: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static> ErrorOf<K> {
    pub fn new(kind: K, message: impl Into<Arc<str>>) -> Self {
        let message = message.into();
        Self { kind, message }
    }
}

impl<Kind: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static> HasErrorKind<Kind>
    for ErrorOf<Kind>
{
    fn kind(&self) -> Kind {
        self.kind
    }
}
