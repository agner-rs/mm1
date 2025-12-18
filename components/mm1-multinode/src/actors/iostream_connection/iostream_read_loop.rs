use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_proto::message;
use mm1_proto_network_management::protocols::ForeignTypeKey;
use tokio::io::AsyncRead;

use super::*;
use crate::actors::context::ActorContext;
use crate::common::RouteMetric;

#[message(base_path = ::mm1_proto)]
pub(super) struct DeclareType {
    pub(super) foreign_type_key: ForeignTypeKey,
    pub(super) name:             Arc<str>,
}

#[message(base_path = ::mm1_proto)]
pub(super) struct SubnetDistance {
    pub(super) net_address: NetAddress,
    pub(super) type_handle: ForeignTypeKey,
    pub(super) metric:      Option<RouteMetric>,
}

#[message(base_path = ::mm1_proto)]
pub(super) struct ReceivedMessage {
    pub(super) dst_address:      Address,
    pub(super) trace_id:         ::mm1_core::tracing::TraceId,
    pub(super) origin_seq_no:    u64,
    pub(super) ttl:              u8,
    pub(super) priority:         bool,
    pub(super) foreign_type_key: ForeignTypeKey,
    pub(super) body:             Box<[u8]>,
}

pub(super) async fn run<Ctx, R>(mut ctx: Ctx, io: R, report_to: Address) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
    R: AsyncRead,
{
    use pdu::Header as H;

    let mut io = pin!(io);

    loop {
        let header = iostream_util::read_header(&mut io)
            .await
            .wrap_err("read_header")?;
        match header {
            H::Hello(_unexpected_hello) => return Err(eyre::format_err!("unexpected hello")),

            H::KeepAlive => {},

            H::DeclareType(declare_type) => {
                let pdu::DeclareType {
                    message_type,
                    type_name_len,
                } = declare_type;
                let mut buf = vec![0u8; type_name_len as usize];
                io.read_exact(&mut buf[..]).await.wrap_err("read body")?;
                let type_name = String::from_utf8(buf).wrap_err("non UTF-8 name")?;
                info!(
                    f_key = ?declare_type.message_type,
                    name = %type_name,
                    "type declared"
                );

                let message = DeclareType {
                    foreign_type_key: message_type,
                    name:             type_name.into(),
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },
            H::SubnetDistance(subnet_distance) => {
                let pdu::SubnetDistance {
                    net_address,
                    type_handle,
                    metric,
                } = subnet_distance;
                info!(
                    net = %net_address, f_key = ?type_handle, metric = ?metric,
                    "foreign subnet"
                );

                let message = SubnetDistance {
                    net_address,
                    type_handle,
                    metric,
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },

            H::TransmitMessage(transmit_message) => {
                let pdu::TransmitMessage {
                    dst_address,
                    trace_id,
                    origin_seq_no,
                    message_type,
                    payload_size,
                    ttl,
                    priority,
                } = transmit_message;
                let mut buf = vec![0u8; payload_size as usize].into_boxed_slice();
                let _ = io
                    .read_exact(&mut buf[..])
                    .await
                    .wrap_err("io.read_exact (read body)")?;

                let message = ReceivedMessage {
                    dst_address,
                    trace_id,
                    origin_seq_no,
                    ttl,
                    priority,
                    foreign_type_key: message_type,
                    body: buf,
                };
                ctx.tell(report_to, message).await.wrap_err("ctx.tell")?;
            },
        }
    }
}
