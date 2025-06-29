use std::sync::Arc;

use mm1_proto::message;

use crate::errors::error_kind::{ErrorKind, HasErrorKind};

#[derive(Debug, thiserror::Error)]
#[error("{}: {}", kind, message)]
#[message(base_path = ::mm1_proto)]
pub struct ErrorOf<Kind: ErrorKind> {
    pub kind:    Kind,
    pub message: Arc<str>,
}

impl<K0> ErrorOf<K0>
where
    K0: ErrorKind,
{
    pub fn map_kind<K1>(self, map: impl FnOnce(K0) -> K1) -> ErrorOf<K1>
    where
        K1: ErrorKind,
    {
        let ErrorOf { kind, message } = self;
        let kind = map(kind);
        ErrorOf { kind, message }
    }
}

impl<K: ErrorKind> ErrorOf<K> {
    pub fn new(kind: K, message: impl Into<Arc<str>>) -> Self {
        let message = message.into();
        Self { kind, message }
    }
}

impl<Kind: ErrorKind> HasErrorKind<Kind> for ErrorOf<Kind> {
    fn kind(&self) -> Kind {
        self.kind
    }
}
