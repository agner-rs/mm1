use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[message]
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegisterRequest<K> {
    pub key:       K,
    pub addr:      Address,
    pub key_props: KeyProps,
    pub reg_props: RegProps,
}

pub type RegisterResponse = Result<(), ErrorOf<RegisterErrorKind>>;

#[message]
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UnregisterRequest<K> {
    pub key:  K,
    pub addr: Address,
}

pub type UnregisterResponse = Result<(), ErrorOf<UnregisterErrorKind>>;

#[message]
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResolveRequest<K> {
    pub key: K,
}

pub type ResolveResponse = Result<Vec<(Address, Duration)>, ErrorOf<ResolveErrorKind>>;

#[message]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RegisterErrorKind {
    Conflict,
    Internal,
}

#[message]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UnregisterErrorKind {
    NotFound,
    Internal,
}

#[message]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResolveErrorKind {
    Internal,
}

#[message]
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyProps {
    pub exclusive: bool,
}

#[message]
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegProps {
    pub ttl: Duration,
}

impl_error_kind!(RegisterErrorKind);
impl_error_kind!(UnregisterErrorKind);
impl_error_kind!(ResolveErrorKind);
