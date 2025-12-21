use std::fs::File;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eyre::Context;
use fs2::FileExt;
use mm1_address::address::Address;
use mm1_common::log::{error, info, warn};
use mm1_common::types::{AnyError, Never};
use mm1_core::envelope::{Envelope, dispatch};
use mm1_core::tracing::{TraceId, WithTraceIdExt};
use mm1_proto_network_management::protocols::ProtocolResolved;
use mm1_proto_network_management::{self as nm, protocols};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_well_known::MULTINODE_MANAGER;
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};

use crate::actors::context::ActorContext;
use crate::codec::Protocol;

const PROTOCOL_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_START_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    bind_addr: Box<Path>,
    protocol_names: Vec<nm::ProtocolName>,
    options: nm::Options,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    ctx.init_done(ctx.address()).await;

    let mut protocols = vec![];
    for protocol_name in protocol_names {
        let protocol = wait_for_protocol(ctx, protocol_name)
            .await
            .wrap_err("wait_for_protocol")?;
        protocols.push(protocol);
    }

    let _lock_file =
        try_cleanup_stale_socket(bind_addr.as_ref()).wrap_err("try_cleanup_stale_socket")?;
    let uds_listener = RmOnDrop::new(
        bind_addr.as_ref().to_path_buf(),
        UnixListener::bind(bind_addr.as_ref()).wrap_err("UnixListener::bind")?,
    );

    event_loop(
        ctx,
        connection_sup,
        &uds_listener,
        Arc::new(options),
        protocols.into_boxed_slice().into(),
    )
    .await
}

async fn wait_for_protocol<Ctx>(
    ctx: &mut Ctx,
    name: nm::ProtocolName,
) -> Result<protocols::ProtocolResolved<Protocol>, AnyError>
where
    Ctx: ActorContext,
{
    let found = ctx
        .ask::<_, protocols::GetProtocolByNameResponse<Protocol>>(
            MULTINODE_MANAGER,
            protocols::GetProtocolByNameRequest {
                name,
                timeout: Some(PROTOCOL_WAIT_TIMEOUT),
            },
            PROTOCOL_WAIT_TIMEOUT + Duration::from_secs(1),
        )
        .await
        .wrap_err("ask")?
        .wrap_err("GetProtocolByName")?;

    Ok(found)
}

async fn event_loop<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    uds_listener: &UnixListener,
    options: Arc<nm::Options>,
    protocols: Arc<[ProtocolResolved<Protocol>]>,
) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    loop {
        let accepted = uds_listener.accept();
        let received = ctx.recv();

        tokio::select! {
            accept_result = accepted => {
                let (uds_stream, peer_addr) = accept_result.wrap_err("uds_listener.accept")?;
                let trace_id = TraceId::random();
                handle_accepted(ctx, connection_sup, options.clone(), protocols.clone(), uds_stream, peer_addr).with_trace_id(trace_id).await.wrap_err("handle_accepted")?;
            },
            recv_result = received => {
                let envelope = recv_result.wrap_err("ctx.recv")?;
                let trace_id = envelope.header().trace_id();
                handle_envelope(ctx, envelope).with_trace_id(trace_id).await.wrap_err("handle_envelope")?;
            }
        }
    }
}

async fn handle_accepted<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
    options: Arc<nm::Options>,
    protocols: Arc<[ProtocolResolved<Protocol>]>,
    uds_stream: UnixStream,
    peer_addr: SocketAddr,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    let local_addr = uds_stream.local_addr().wrap_err("uds_stream.local_addr")?;
    info!(
        peer = ?peer_addr, local = ?local_addr,
        "accepted a connection"
    );

    let connection_addr = ctx
        .ask::<_, uni_sup::StartResponse>(
            connection_sup,
            uni_sup::StartRequest {
                args: (uds_stream, options, protocols),
            },
            CONNECTION_START_TIMEOUT,
        )
        .await
        .wrap_err("ask")?
        .wrap_err("uni_sup::Start")?;
    info!(
        %connection_addr, peer = ?peer_addr, local = ?local_addr,
        "connection started"
    );

    Ok(())
}

async fn handle_envelope<Ctx>(_ctx: &mut Ctx, envelope: Envelope) -> Result<(), AnyError> {
    dispatch!(match envelope {
        unexpected @ _ => error!(?unexpected, "received unexpected message"),
    });
    Ok(())
}

fn try_cleanup_stale_socket(socket_path: &Path) -> Result<RmOnDrop<File>, AnyError> {
    let mut lock_path = socket_path.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock_path = PathBuf::from(lock_path);

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .wrap_err("open lock file")?;

    if let Err(err) = lock_file.try_lock_exclusive() {
        error!(
            socket = ?socket_path,
            lock = ?lock_path,
            error_kind = ?err.kind(),
            ?err,
            "failed to acquire lock on socket, already in use"
        );
        return Err(err).wrap_err("socket already in use");
    }

    if socket_path.exists() {
        warn!(socket = ?socket_path, "removing stale socket");
        std::fs::remove_file(socket_path).wrap_err("remove stale socket")?;
    }

    Ok(RmOnDrop::new(lock_path, lock_file))
}

struct RmOnDrop<F> {
    path:  PathBuf,
    inner: F,
}

impl<F> RmOnDrop<F> {
    fn new(path: impl Into<PathBuf>, inner: F) -> Self {
        Self {
            path: path.into(),
            inner,
        }
    }
}

impl<F> Deref for RmOnDrop<F> {
    type Target = F;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<F> DerefMut for RmOnDrop<F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<F> Drop for RmOnDrop<F> {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_file(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            error!(path = ?self.path, ?err, "failed to remove socket");
        }
    }
}
