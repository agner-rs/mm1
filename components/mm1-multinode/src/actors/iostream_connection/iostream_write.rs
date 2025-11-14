use std::any::TypeId;
use std::collections::HashMap;
use std::collections::hash_map::Entry::*;
use std::pin::pin;

use eyre::Context;
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_common::types::AnyError;
use mm1_core::envelope::EnvelopeHeader;
use mm1_core::message::AnyMessage;
use mm1_proto_network_management as nm;
use mm1_proto_network_management::protocols as p;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::actors::context::ActorContext;
use crate::actors::iostream_connection::{iostream_util, mn_mgr, pdu};
use crate::codec::ErasedCodec;
use crate::proto;

pub(super) struct OutputWriter<Ctx, W> {
    ctx:                 Ctx,
    io_w:                W,
    multinode_manager:   Address,
    declared_types:      HashMap<p::LocalTypeKey, nm::MessageName>,
    outbound_by_type_id: HashMap<TypeId, (p::LocalTypeKey, ErasedCodec)>,
}

impl<Ctx, W> OutputWriter<Ctx, W>
where
    Ctx: ActorContext,
    W: AsyncWrite + Unpin,
{
    pub(super) fn new(
        ctx: Ctx,
        io_w: W,
        multinode_manager: Address,
        outbound_by_type_id: HashMap<TypeId, (p::LocalTypeKey, ErasedCodec)>,
    ) -> Self {
        Self {
            ctx,
            io_w,
            multinode_manager,
            declared_types: Default::default(),
            outbound_by_type_id,
        }
    }

    pub(super) async fn write_keep_alive(&mut self) -> Result<(), AnyError> {
        let Self { io_w, .. } = self;
        let mut io_w = pin!(io_w);
        iostream_util::write_header(&mut io_w, pdu::Header::KeepAlive)
            .await
            .wrap_err("iostream_util::write_header")?;
        io_w.flush().await.wrap_err("io_w.flush")?;
        Ok(())
    }

    pub(super) async fn write_subnet_distance(
        &mut self,
        route: &proto::SetRoute,
    ) -> Result<(), AnyError> {
        let Self {
            ctx,
            io_w,
            multinode_manager,
            declared_types,
            ..
        } = self;
        let proto::SetRoute {
            message,
            destination,
            via: _,
            metric,
        } = route;

        let mut io_w = pin!(io_w);

        if let Vacant(message_to_declare) = declared_types.entry(*message) {
            let name = mn_mgr::get_message_name(ctx, *multinode_manager, *message)
                .await
                .wrap_err("mn_mgr::get_message_name")?;
            do_write_declare_type(&mut io_w, *message, name.clone())
                .await
                .wrap_err("do_write_declared_type")?;
            message_to_declare.insert(name);
        }
        do_write_subnet_distance(&mut io_w, *destination, *message, *metric)
            .await
            .wrap_err("do_write_subnet_distance")?;

        Ok(())
    }

    pub(super) async fn write_delcare_type(
        &mut self,
        message_type: p::LocalTypeKey,
        name: nm::MessageName,
    ) -> Result<(), AnyError> {
        let Self {
            io_w,
            declared_types,
            ..
        } = self;
        match declared_types.entry(message_type) {
            Occupied(message_declared) if message_declared.get() == &name => Ok(()),
            Occupied(message_declared) => {
                Err(eyre::format_err!(
                    "attempt to declare the same l-key: {:?} with two different names [old: {}; \
                     new: {}]",
                    message_type,
                    message_declared.get(),
                    name
                ))
            },
            Vacant(message_to_declare) => {
                do_write_declare_type(io_w, message_type, name.clone())
                    .await
                    .wrap_err("do_write_declare_type")?;
                message_to_declare.insert(name);
                Ok(())
            },
        }
    }

    pub(super) async fn write_known_message(
        &mut self,
        envelope_header: &EnvelopeHeader,
        message: AnyMessage,
    ) -> Result<(), AnyError> {
        debug_assert!(message.peek::<proto::Forward>().is_none());

        let Self {
            io_w,
            declared_types,
            outbound_by_type_id,
            ..
        } = self;

        let mut io_w = pin!(io_w);

        let tid = message.tid();
        let &(message_type, ref codec) = outbound_by_type_id
            .get(&tid)
            .ok_or_else(|| eyre::format_err!("no codec for {}", message.type_name()))?;

        assert!(declared_types.contains_key(&message_type));

        let mut buf: Vec<u8> = vec![];
        codec.encode(&message, &mut buf).wrap_err("codec::encode")?;
        let body = buf.into_boxed_slice();

        let payload_size = body.len().try_into().wrap_err("message too large")?;

        let header = pdu::TransmitMessage {
            // FIXME: other Header information is erased here
            dst_address: envelope_header.to,
            message_type,
            payload_size,
        };
        iostream_util::write_header(&mut io_w, header)
            .await
            .wrap_err("iostream_util::write_header")?;
        io_w.write_all(&body).await.wrap_err("io_w.write_all")?;
        io_w.flush().await.wrap_err("io_w.flush")?;

        Ok(())
    }

    pub(super) async fn write_opaque_message(
        &mut self,
        envelope_header: &EnvelopeHeader,
        to_forward: proto::Forward,
    ) -> Result<(), AnyError> {
        let Self {
            ctx,
            io_w,
            multinode_manager,
            declared_types,
            ..
        } = self;
        let proto::Forward {
            local_type_key: message_type,
            body,
        } = to_forward;

        let mut io_w = pin!(io_w);

        if let Vacant(message_to_declare) = declared_types.entry(message_type) {
            // XXX: shouldn't be like that, right? emit a warning?
            let name = mn_mgr::get_message_name(ctx, *multinode_manager, message_type)
                .await
                .wrap_err("mn_mgr::get_message_name")?;
            do_write_declare_type(&mut io_w, message_type, name.clone())
                .await
                .wrap_err("do_write_declared_type")?;
            message_to_declare.insert(name);
        }

        let payload_size = body.len().try_into().wrap_err("message too large")?;

        let header = pdu::TransmitMessage {
            // FIXME: other Header information is erased here
            dst_address: envelope_header.to,
            message_type,
            payload_size,
        };
        iostream_util::write_header(&mut io_w, header)
            .await
            .wrap_err("iostream_util::write_header")?;
        io_w.write_all(&body).await.wrap_err("io_w.write_all")?;
        io_w.flush().await.wrap_err("io_w.flush")?;

        Ok(())
    }
}

async fn do_write_declare_type<W>(
    io_w: W,
    message_type: p::LocalTypeKey,
    name: nm::MessageName,
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let mut io_w = pin!(io_w);

    let type_name_len = name
        .len()
        .try_into()
        .wrap_err("message name is too long?")?;
    let header = pdu::DeclareType {
        message_type,
        type_name_len,
    };
    iostream_util::write_header(&mut io_w, header)
        .await
        .wrap_err("iostream_util::write_header")?;
    io_w.write_all(name.as_bytes())
        .await
        .wrap_err("io_w.write_all")?;
    io_w.flush().await.wrap_err("io_w.flush")?;

    Ok(())
}

async fn do_write_subnet_distance<W>(
    io_w: W,
    net_address: NetAddress,
    type_handle: p::LocalTypeKey,
    metric: Option<u8>,
) -> Result<(), AnyError>
where
    W: AsyncWrite + Unpin,
{
    let header = pdu::SubnetDistance {
        net_address,
        type_handle,
        metric,
    };
    iostream_util::write_header(io_w, header)
        .await
        .wrap_err("iostream_util::write_header")?;
    Ok(())
}
