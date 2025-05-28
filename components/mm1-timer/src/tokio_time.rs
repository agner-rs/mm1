use std::hash::Hash;
use std::marker::PhantomData;
use std::time::Duration;

use mm1_core::prim::Message;
use mm1_proto_timer::Timer;

pub struct TokioTimer<K, M> {
    _key: PhantomData<K>,
    _msg: PhantomData<M>,
}

impl<K, M> Timer for TokioTimer<K, M>
where
    K: Hash + Ord + Clone + Send + 'static,
    M: Message,
{
    type Duration = Duration;
    type Instant = tokio::time::Instant;
    type Key = K;
    type Message = M;

    async fn sleep(d: Self::Duration) {
        tokio::time::sleep(d).await;
    }
}
