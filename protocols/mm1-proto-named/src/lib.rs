use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

#[message(base_path = ::mm1_proto)]
#[derive(Debug)]
pub struct RegisterRequest<K> {
    pub key:       K,
    pub addr:      Address,
    pub key_props: KeyProps,
    pub reg_props: RegProps,
}

pub type RegisterResponse = Result<(), ErrorOf<RegisterErrorKind>>;

#[message(base_path = ::mm1_proto)]
#[derive(Debug)]
pub struct UnregisterRequest<K> {
    pub key:  K,
    pub addr: Address,
}

pub type UnregisterResponse = Result<(), ErrorOf<UnregisterErrorKind>>;

#[message(base_path = ::mm1_proto)]
#[derive(Debug)]
pub struct ResolveRequest<K> {
    pub key: K,
}

pub type ResolveResponse = Result<Vec<(Address, Duration)>, ErrorOf<ResolveErrorKind>>;

#[message(base_path = ::mm1_proto)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegisterErrorKind {
    Conflict,
    Internal,
}

#[message(base_path = ::mm1_proto)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnregisterErrorKind {
    NotFound,
    Internal,
}

#[message(base_path = ::mm1_proto)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolveErrorKind {
    Internal,
}

#[message(base_path = ::mm1_proto)]
#[derive(Debug)]
pub struct KeyProps {
    pub exclusive: bool,
}

#[message(base_path = ::mm1_proto)]
#[derive(Debug)]
pub struct RegProps {
    pub ttl: Duration,
}

impl_error_kind!(RegisterErrorKind);
impl_error_kind!(UnregisterErrorKind);
impl_error_kind!(ResolveErrorKind);
