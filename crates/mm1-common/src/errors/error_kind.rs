use std::fmt;

use crate::types::Never;

pub trait HasErrorKind<Kind: ErrorKind> {
    fn kind(&self) -> Kind;
}

pub trait ErrorKind: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static {}

impl<AnyKind> HasErrorKind<AnyKind> for Never
where
    AnyKind: ErrorKind,
{
    fn kind(&self) -> AnyKind {
        match *self {}
    }
}

impl<K> ErrorKind for K where K: fmt::Display + Eq + Ord + Copy + Send + Sync + 'static {}

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
