use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use metrics::{Counter, Histogram};

#[macro_export]
macro_rules! make_metrics {
    ($name:literal $(, $k:literal => $v:expr )* $(,)?) => {{
        const _: &str = $name;
        const POLL_COUNT: &str = concat!($name, "_poll_count");
        const DONE_COUNT: &str = concat!($name, "_done_count");
        const BUSY_TIME: &str = concat!($name, "_busy_sec");
        const WAIT_TIME: &str = concat!($name, "_wait_sec");
        const WALL_TIME: &str = concat!($name, "_wall_sec");

        let poll_count = ::metrics::counter!(POLL_COUNT $(, $k => $v )*);
        let done_count = ::metrics::counter!(DONE_COUNT $(, $k => $v )*);
        let busy_time = ::metrics::histogram!(BUSY_TIME $(, $k => $v )*);
        let wait_time = ::metrics::histogram!(WAIT_TIME $(, $k => $v )*);
        let wall_time = ::metrics::histogram!(WALL_TIME $(, $k => $v )*);

        $crate::metrics::Metrics {
            poll_count,
            done_count,
            busy_time,
            wait_time,
            wall_time,
        }
    }};
}

pub trait MeasuredFutureExt: Future + Sized {
    fn measured(self, metrics: Metrics) -> MeasuredFuture<Self> {
        MeasuredFuture::new(self, metrics)
    }
}
impl<F> MeasuredFutureExt for F where F: Future + Sized {}

#[pin_project::pin_project]
#[derive(derive_more::Debug)]
pub struct MeasuredFuture<F> {
    #[debug(skip)]
    #[pin]
    inner: F,

    metrics:      Metrics,
    measurements: Measurements,
}

#[derive(Debug)]
pub struct Metrics {
    pub poll_count: Counter,
    pub done_count: Counter,
    pub busy_time:  Histogram,
    pub wait_time:  Histogram,
    pub wall_time:  Histogram,
}

#[derive(Default, Debug)]
struct Measurements {
    first_poll: Option<Instant>,
    last_poll:  Option<Instant>,
    busy_time:  Duration,
    wait_time:  Duration,
}

impl<F> MeasuredFuture<F> {
    pub fn new(inner: F, metrics: Metrics) -> Self {
        Self {
            inner,
            metrics,
            measurements: Default::default(),
        }
    }
}

impl<F> Future for MeasuredFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        this.metrics.poll_count.increment(1);

        let t_start = Instant::now();
        if let Some(last_poll) = this.measurements.last_poll {
            this.measurements.wait_time += t_start.duration_since(last_poll);
        }
        let first_poll = this.measurements.first_poll.get_or_insert(t_start);

        let poll = this.inner.poll(cx);

        let t_end = Instant::now();
        this.measurements.busy_time += t_end.duration_since(t_start);

        if poll.is_ready() {
            this.metrics
                .wall_time
                .record(t_end.duration_since(*first_poll));
            this.metrics.busy_time.record(this.measurements.busy_time);
            this.metrics.wait_time.record(this.measurements.wait_time);

            this.metrics.done_count.increment(1);
        }

        poll
    }
}
