use std::any::TypeId;
use std::sync::Arc;
use std::time::Duration;

use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::impl_error_kind;
use mm1_proto::message;

use crate::MessageName;

slotmap::new_key_type! {
    pub struct LocalTypeKey;
    pub struct ForeignTypeKey;
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct RegisterOpaqueMessageRequest {
    pub name: crate::MessageName,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct RegisterOpaqueMessageResponse {
    pub key: LocalTypeKey,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct RegisterProtocolRequest<P> {
    pub name:     crate::ProtocolName,
    #[serde(skip)]
    pub protocol: P,
}

pub type RegisterProtocolResponse = Result<(), ErrorOf<RegisterProtocolErrorKind>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum RegisterProtocolErrorKind {
    DuplicateProtocolName,
}

impl_error_kind!(RegisterProtocolErrorKind);

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct UnregisterProtocolRequest {
    pub name: crate::ProtocolName,
}

pub type UnregisterProtocolResponse = Result<(), ErrorOf<UnregisterProtocolErrorKind>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum UnregisterProtocolErrorKind {
    NoProtocol,
    ProtocolInUse,
}

impl_error_kind!(UnregisterProtocolErrorKind);

// TODO: move to routing
#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct RegisterLocalSubnetRequest {
    pub net: NetAddress,
}

pub type RegisterLocalSubnetResponse = Result<(), ErrorOf<RegisterLocalSubnetErrorKind>>;

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct GetLocalSubnetsRequest;

pub type GetLocalSubnetsResponse = Vec<NetAddress>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum RegisterLocalSubnetErrorKind {}

impl_error_kind!(RegisterLocalSubnetErrorKind);

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct GetProtocolByNameRequest {
    pub name:    crate::ProtocolName,
    pub timeout: Option<Duration>,
}

#[derive(Debug)]
#[message(base_path = ::mm1_proto)]
pub struct ProtocolResolved<P> {
    #[serde(skip)]
    pub protocol: Arc<P>,

    pub outbound: Vec<(MessageName, LocalTypeKey)>,
    pub inbound:  Vec<(MessageName, LocalTypeKey)>,
}

pub type GetProtocolByNameResponse<P> =
    Result<ProtocolResolved<P>, ErrorOf<GetProtocolByNameErrorKind>>;

#[message(base_path = ::mm1_proto)]
pub struct ResolveTypeIdRequest {
    #[serde(with = "no_serde")]
    pub type_id: TypeId,
}

#[message(base_path = ::mm1_proto)]
pub struct ResolveTypeIdResponse {
    pub type_key_opt: Option<LocalTypeKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[message(base_path = ::mm1_proto)]
pub enum GetProtocolByNameErrorKind {
    NoProtocol,
}

impl_error_kind!(GetProtocolByNameErrorKind);

mod no_serde {
    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S, T>(_: T, _: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reason = S::Error::custom("not supported");
        Err(reason)
    }
    pub(super) fn deserialize<'de, D, T>(_: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
    {
        let reason = D::Error::custom("not supported");
        Err(reason)
    }
}
