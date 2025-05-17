use std::future::IntoFuture;
use std::time::Duration;

use tokio::time;

pub trait FutureTimeoutExt: IntoFuture + Sized {
    fn timeout(self, duration: Duration) -> time::Timeout<Self::IntoFuture> {
        time::timeout(duration, self)
    }
}

impl<F> FutureTimeoutExt for F where F: IntoFuture {}
