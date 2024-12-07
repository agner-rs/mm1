use std::fmt;

use crate::types::Never;

pub trait HasErrorKind<Kind: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static> {
    fn kind(&self) -> Kind;
}

impl<AnyKind> HasErrorKind<AnyKind> for Never
where
    AnyKind: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static,
{
    fn kind(&self) -> AnyKind {
        match *self {}
    }
}

#[macro_export]
macro_rules! impl_error_kind {
    ($ty:ty) => {
        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }
        impl std::error::Error for $ty {}
        impl $crate::errors::error_kind::HasErrorKind<Self> for $ty {
            fn kind(&self) -> Self {
                *self
            }
        }
    };
}
