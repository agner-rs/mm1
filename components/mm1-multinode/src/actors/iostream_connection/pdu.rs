use std::mem;

use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_core::tracing::TraceId;
pub use mm1_proto_network_management::protocols::{ForeignTypeKey, LocalTypeKey};
use serde::{Deserialize, Serialize};

use crate::common::RouteMetric;

static_assertions::assert_eq_size!(Header<LocalTypeKey>, [u8; 48]);
static_assertions::assert_eq_align!(Header<LocalTypeKey>, u64);
static_assertions::assert_eq_size!(Header<ForeignTypeKey>, [u8; 48]);
static_assertions::assert_eq_align!(Header<ForeignTypeKey>, u64);
static_assertions::assert_eq_size!(LocalTypeKey, u64);
static_assertions::assert_eq_size!(ForeignTypeKey, u64);

pub(crate) const HEADER_FRAME_SIZE: usize = mem::size_of::<Header<u64>>();

#[derive(Debug, Clone, Copy, Serialize, Deserialize, derive_more::From)]
#[repr(C)]
pub(crate) enum Header<TypeKey> {
    Hello(Hello),
    KeepAlive,
    SubnetDistance(SubnetDistance<TypeKey>),
    DeclareType(DeclareType<TypeKey>),
    TransmitMessage(TransmitMessage<TypeKey>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C)]
pub(crate) struct Hello(pub(crate) [u8; 30]);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C)]
pub(crate) struct SubnetDistance<TypeKey> {
    pub(crate) net_address: NetAddress,
    pub(crate) type_handle: TypeKey,
    pub(crate) metric:      Option<RouteMetric>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C)]
pub(crate) struct DeclareType<TypeKey> {
    pub(crate) message_type:  TypeKey,
    pub(crate) type_name_len: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C)]
pub(crate) struct TransmitMessage<TypeKey> {
    pub(crate) dst_address:   Address,
    pub(crate) message_type:  TypeKey,
    pub(crate) trace_id:      TraceId,
    pub(crate) origin_seq_no: u64,
    pub(crate) payload_size:  u16,
    pub(crate) ttl:           u8,
    pub(crate) priority:      bool,
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    #[test_case(Hello(Default::default()).into())]
    #[test_case(SubnetDistance { net_address: "<a:1>/64".parse().unwrap(), type_handle: 222, metric: Some(1) }.into())]
    #[test_case(SubnetDistance { net_address: "<a:1>/64".parse().unwrap(), type_handle: 222, metric: None }.into())]
    #[test_case(DeclareType { message_type: u64::MAX, type_name_len: 222, }.into())]
    #[test_case(TransmitMessage { dst_address: "<a:1>".parse().unwrap(), trace_id: Default::default(), message_type: 222, origin_seq_no: 0, payload_size: 55555, ttl: 5, priority: false }.into())]
    fn encoded_header_fits_into_header_frame(input: Header<u64>) {
        let mut buf = [128u8; HEADER_FRAME_SIZE];
        bincode::serde::encode_into_slice(input, buf.as_mut(), bincode::config::standard())
            .expect("encode");
        let (..): (Header<u64>, _) =
            bincode::serde::decode_from_slice(buf.as_ref(), bincode::config::standard())
                .expect("decode");
    }
}
