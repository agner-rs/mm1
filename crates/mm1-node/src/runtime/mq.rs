use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use tokio::sync::mpsc::error::{SendError, TrySendError};
use tokio::sync::mpsc::{self};

pub(crate) fn unbounded<T>() -> (UbTx<T>, UbRx<T>) {
    let count = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::unbounded_channel();
    let tx = UbTx {
        count: count.clone(),
        tx,
    };
    let rx = UbRx { count, rx };
    (tx, rx)
}

pub(crate) fn bounded<T>(max: usize) -> (Tx<T>, Rx<T>) {
    let count = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel(max);
    let tx = Tx {
        count: count.clone(),
        tx,
    };
    let rx = Rx { count, rx };
    (tx, rx)
}

#[derive(Debug)]
pub(crate) struct UbTx<T> {
    count: Arc<AtomicUsize>,
    tx:    mpsc::UnboundedSender<T>,
}

#[derive(Debug)]
pub(crate) struct UbTxWeak<T> {
    count: Arc<AtomicUsize>,
    tx:    mpsc::WeakUnboundedSender<T>,
}

pub(crate) struct UbRx<T> {
    count: Arc<AtomicUsize>,
    rx:    mpsc::UnboundedReceiver<T>,
}

#[derive(Debug)]
pub(crate) struct Tx<T> {
    count: Arc<AtomicUsize>,
    tx:    mpsc::Sender<T>,
}

// #[derive(Debug)]
// pub(crate) struct TxWeak<T> {
//     count: Arc<AtomicUsize>,
//     tx: mpsc::WeakSender<T>,
// }

pub(crate) struct Rx<T> {
    count: Arc<AtomicUsize>,
    rx:    mpsc::Receiver<T>,
}

impl<T> UbTx<T> {
    pub(crate) fn send(&self, message: T) -> Result<usize, SendError<T>> {
        let out = self.count.fetch_add(1, AtomicOrdering::Relaxed) + 1;
        self.tx.send(message).map(|()| out)
    }

    pub(crate) fn downgrade(&self) -> UbTxWeak<T> {
        UbTxWeak {
            count: self.count.clone(),
            tx:    self.tx.downgrade(),
        }
    }
}

impl<T> UbTxWeak<T> {
    pub(crate) fn upgrade(&self) -> Option<UbTx<T>> {
        Some(UbTx {
            count: self.count.clone(),
            tx:    self.tx.upgrade()?,
        })
    }
}

impl<T> UbRx<T> {
    pub(crate) async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await.inspect(|_| {
            self.count.fetch_sub(1, AtomicOrdering::Relaxed);
        })
    }

    pub(crate) fn close(&mut self) {
        self.rx.close();
    }
}

impl<T> Tx<T> {
    pub(crate) fn try_send(&self, message: T) -> Result<usize, TrySendError<T>> {
        let out = self.count.fetch_add(1, AtomicOrdering::Relaxed) + 1;
        self.tx.try_send(message).map(|()| out)
    }

    // pub(crate) fn downgrade(&self) -> TxWeak<T> {
    //     TxWeak {
    //         count: self.count.clone(),
    //         tx: self.tx.downgrade(),
    //     }
    // }
}

// impl<T> TxWeak<T> {
//     pub(crate) fn upgrade(&self) -> Option<Tx<T>> {
//         Some(Tx {
//             count: self.count.clone(),
//             tx: self.tx.upgrade()?,
//         })
//     }
// }

impl<T> Rx<T> {
    pub(crate) async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await.inspect(|_| {
            self.count.fetch_sub(1, AtomicOrdering::Relaxed);
        })
    }

    pub(crate) fn close(&mut self) {
        self.rx.close();
    }
}

impl<T> Clone for UbTx<T> {
    fn clone(&self) -> Self {
        Self {
            count: self.count.clone(),
            tx:    self.tx.clone(),
        }
    }
}

impl<T> Clone for UbTxWeak<T> {
    fn clone(&self) -> Self {
        Self {
            count: self.count.clone(),
            tx:    self.tx.clone(),
        }
    }
}

impl<T> Clone for Tx<T> {
    fn clone(&self) -> Self {
        Self {
            count: self.count.clone(),
            tx:    self.tx.clone(),
        }
    }
}

// impl<T> Clone for TxWeak<T> {
//     fn clone(&self) -> Self {
//         Self {
//             count: self.count.clone(),
//             tx: self.tx.clone(),
//         }
//     }
// }
