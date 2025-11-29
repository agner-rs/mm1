use std::fmt;

use rand::RngCore;
use tokio::task_local;
use tracing::{Instrument, Level, Span, span};

task_local! {
    static TRACE_ID: TraceId;
}

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct TraceId(u64);

pub trait WithTraceIdExt: Future + Sized {
    fn with_trace_id(self, trace_id: TraceId) -> impl Future<Output = Self::Output> {
        trace_id.scope_async(self)
    }
}

impl<F> WithTraceIdExt for F where F: Future + Sized {}

impl TraceId {
    pub fn random() -> Self {
        rand::rng().next_u64().into()
    }

    pub fn current() -> Self {
        TRACE_ID.try_get().ok().unwrap_or_default()
    }

    pub fn scope_async<F>(self, fut: F) -> impl Future<Output = F::Output>
    where
        F: Future,
    {
        TRACE_ID.scope(self, fut.instrument(self.span()))
    }

    pub fn scope_sync<F, R>(self, func: F) -> R
    where
        F: FnOnce() -> R,
    {
        TRACE_ID.sync_scope(self, || self.span().in_scope(func))
    }

    pub fn span(self) -> Span {
        span!(
            Level::TRACE,
            "trace-id-async-scope",
            trace_id = tracing::field::display(self)
        )
    }
}

impl From<u64> for TraceId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
impl From<TraceId> for u64 {
    fn from(value: TraceId) -> Self {
        let TraceId(value) = value;
        value
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T#{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::tracing::TraceId;

    #[tokio::test]
    async fn scopes() {
        let t_0 = TraceId::current();
        let t_1 = TraceId::random();

        assert_ne!(t_0, t_1);

        assert_eq!(t_0, t_0.scope_async(async { TraceId::current() }).await);
        assert_eq!(t_0, t_0.scope_sync(TraceId::current));

        assert_eq!(t_1, t_1.scope_async(async { TraceId::current() }).await);
        assert_eq!(t_1, t_1.scope_sync(TraceId::current));
    }
}
