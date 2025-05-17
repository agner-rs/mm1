use async_channel as mpmc;
pub(crate) use async_channel::{SendError, TrySendError};

pub(crate) fn unbounded<T>() -> (UbTx<T>, UbRx<T>) {
    let (tx, rx) = mpmc::unbounded();
    let tx = UbTx { tx };

    let rx = UbRx { rx };
    (tx, rx)
}

pub(crate) fn bounded<T>(max: usize) -> (Tx<T>, Rx<T>) {
    let (tx, rx) = mpmc::bounded(max);

    let tx = Tx { tx };
    let rx = Rx { rx };
    (tx, rx)
}

#[derive(Debug)]
pub(crate) struct UbTx<T> {
    tx: mpmc::Sender<T>,
}

#[derive(Debug)]
pub(crate) struct UbTxWeak<T> {
    tx: mpmc::WeakSender<T>,
}

pub(crate) struct UbRx<T> {
    rx: mpmc::Receiver<T>,
}

#[derive(Debug)]
pub(crate) struct Tx<T> {
    tx: mpmc::Sender<T>,
}

// #[derive(Debug)]
// pub(crate) struct TxWeak<T> {
//     tx: mpmc::WeakSender<T>,
// }

pub(crate) struct Rx<T> {
    rx: mpmc::Receiver<T>,
}

impl<T> UbTx<T> {
    pub(crate) fn send(&self, message: T) -> Result<(), SendError<T>> {
        self.tx.try_send(message).map_err(|e| {
            match e {
                TrySendError::Full(_) => panic!(),
                TrySendError::Closed(v) => SendError(v),
            }
        })
    }

    pub(crate) fn downgrade(&self) -> UbTxWeak<T> {
        UbTxWeak {
            tx: self.tx.downgrade(),
        }
    }

    // pub(crate) fn len(&self) -> usize {
    //     self.tx.len()
    // }
}

impl<T> UbTxWeak<T> {
    pub(crate) fn upgrade(&self) -> Option<UbTx<T>> {
        Some(UbTx {
            tx: self.tx.upgrade()?,
        })
    }
}

impl<T> UbRx<T> {
    pub(crate) async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await.ok()
    }

    pub(crate) fn close(&mut self) {
        self.rx.close();
    }
}

impl<T> Tx<T> {
    pub(crate) fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
        self.tx.try_send(message)
    }

    // pub(crate) fn downgrade(&self) -> TxWeak<T> {
    //     TxWeak {
    //         tx: self.tx.downgrade(),
    //     }
    // }

    // pub(crate) fn len(&self) -> usize {
    //     self.tx.len()
    // }
}

// impl<T> TxWeak<T> {
//     pub(crate) fn upgrade(&self) -> Option<Tx<T>> {
//         Some(Tx {
//             tx: self.tx.upgrade()?,
//         })
//     }
// }

impl<T> Rx<T> {
    pub(crate) async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await.ok()
    }

    pub(crate) fn close(&mut self) {
        self.rx.close();
    }
}

impl<T> Clone for UbTx<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T> Clone for UbTxWeak<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T> Clone for Tx<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

// impl<T> Clone for TxWeak<T> {
//     fn clone(&self) -> Self {
//         Self {
//             tx: self.tx.clone(),
//         }
//     }
// }
