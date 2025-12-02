use core::fmt;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use futures::Stream;
use mm1_address::address::Address;
use mm1_common::types::AnyError;
use mm1_core::envelope::Envelope;
use mm1_core::tracing::TraceId;
use mm1_proto_system::WatchRef;
use tokio::sync::{mpsc, oneshot};

pub(crate) fn create() -> (Tx, Rx) {
    let (tx, rx) = mpsc::channel(1);
    (Tx(tx), Rx(rx))
}

pub(crate) enum SysCall {
    Exit(Result<(), AnyError>),
    TrapExit(bool),
    Link {
        sender:   Address,
        receiver: Address,
    },
    Unlink {
        sender:   Address,
        receiver: Address,
    },
    ForkAdded(Address, mpsc::WeakUnboundedSender<Envelope>),
    Spawn(Pin<Box<dyn Future<Output = ()> + Send + 'static>>),
    Watch {
        sender:   Address,
        receiver: Address,
        reply_tx: oneshot::Sender<WatchRef>,
    },
    Unwatch {
        sender:    Address,
        watch_ref: WatchRef,
    },
}

pub(crate) struct Request {
    pub(crate) trace_id: TraceId,
    pub(crate) call:     SysCall,
    #[allow(dead_code)]
    pub(crate) ack_tx:   oneshot::Sender<std::convert::Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) struct Tx(mpsc::Sender<Request>);

#[derive(Debug)]
#[pin_project::pin_project]
pub(crate) struct Rx(#[pin] mpsc::Receiver<Request>);

impl Tx {
    pub(crate) async fn invoke(&self, call: SysCall) {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.0
            .send(Request {
                trace_id: TraceId::current(),
                call,
                ack_tx,
            })
            .await
            .expect("call: tx.send failed");
        let _ = ack_rx.await;
    }
}

impl Stream for Rx {
    type Item = (TraceId, SysCall);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(
            ready!(self.project().0.poll_recv(cx))
                .map(|Request { trace_id, call, .. }| (trace_id, call)),
        )
    }
}

impl fmt::Display for SysCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysCall::Exit(Ok(())) => write!(f, "exit::ok"),
            SysCall::Exit(Err(reason)) => write!(f, "exit::err [reason: {reason}]"),
            SysCall::Link { sender, receiver } => write!(f, "link [{sender} -> {receiver}]"),
            SysCall::Unlink { sender, receiver } => {
                write!(f, "unlink [{sender} -> {receiver}]")
            },
            SysCall::TrapExit(set_into) => write!(f, "trap_exit [set-into: {set_into}]"),
            SysCall::ForkAdded(address, _) => write!(f, "fork_addded [addr: {address}]"),
            SysCall::Spawn(_) => write!(f, "spawn"),
            SysCall::Watch {
                sender, receiver, ..
            } => write!(f, "watch [sender: {sender}, receiver: {receiver}]"),
            SysCall::Unwatch { sender, watch_ref } => {
                write!(f, "unwatch [sender: {sender}, ref: {watch_ref}]")
            },
        }
    }
}

impl fmt::Debug for SysCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
