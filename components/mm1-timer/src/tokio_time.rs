use std::hash::Hash;
use std::marker::PhantomData;
use std::time::Duration;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_core::context::{Fork, Messaging, Now, Quit, Tell};
use mm1_core::prim::Message;
use mm1_proto_timer as t;
use mm1_proto_timer::Timer;

use crate::api::{TimerApi, TimerError};

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

#[cfg(feature = "tokio-time")]
pub async fn new_tokio_timer<K, M, Ctx>(
    ctx: &mut Ctx,
) -> Result<
    impl TimerApi<
        Key = K,
        Message = M,
        Instant = tokio::time::Instant,
        Duration = tokio::time::Duration,
    >,
    TimerError,
>
where
    Ctx: Quit + Fork + Messaging + Now<Instant = tokio::time::Instant>,
    crate::tokio_time::TokioTimer<K, M>: t::Timer<
            Instant = tokio::time::Instant,
            Duration = std::time::Duration,
            Key = K,
            Message = M,
        >,
    K: Hash + Ord + Clone + Send + Sync + 'static,
    M: Message,
{
    TimerApiImpl::<K, M, Ctx, crate::tokio_time::TokioTimer<K, M>>::new(ctx).await
}

struct TimerApiImpl<Key, Msg, Ctx, T> {
    api_context:   Ctx,
    timer_address: Address,
    _pd:           PhantomData<(Key, Msg, T)>,
}

impl<Ctx, Key, Msg, T> TimerApiImpl<Key, Msg, Ctx, T>
where
    Ctx: Quit + Fork + Messaging + Now,
    Key: Hash + Ord + Clone + Send + Sync + 'static,
    Msg: Message,
    T: t::Timer<Key = Key, Message = Msg, Instant = Ctx::Instant>,
    Ctx::Instant: Send,
{
    async fn new(ctx: &mut Ctx) -> Result<Self, TimerError> {
        let receiver = ctx.address();
        let api_context = ctx.fork().await.map_err(TimerError::Fork)?;
        let timer_context = ctx.fork().await.map_err(TimerError::Fork)?;
        let timer = timer_context.address();

        timer_context
            .run(move |mut timer_context| {
                async move {
                    crate::actor::timer_actor::<T, Ctx>(&mut timer_context, receiver)
                        .boxed()
                        .await
                }
            })
            .await;

        let timer_api = Self {
            api_context,
            timer_address: timer,
            _pd: Default::default(),
        };
        Ok(timer_api)
    }
}

impl<Ctx, Key, Msg, T> TimerApi for TimerApiImpl<Key, Msg, Ctx, T>
where
    Ctx: Quit + Fork + Messaging + Now,
    Key: Hash + Ord + Clone + Send + Sync + 'static,
    Msg: Message,
    T: t::Timer<Key = Key, Message = Msg, Instant = Ctx::Instant>,
    Ctx::Instant: Send,
{
    type Duration = T::Duration;
    type Instant = T::Instant;
    type Key = T::Key;
    type Message = T::Message;
    type Timer = T;

    async fn cancel(&mut self, key: Self::Key) -> Result<(), TimerError> {
        self.api_context
            .tell(self.timer_address, t::Cancel::<T> { key })
            .await
            .map_err(TimerError::Send)?;
        Ok(())
    }

    async fn schedule_once_at(
        &mut self,
        key: Self::Key,
        at: Self::Instant,
        msg: Self::Message,
    ) -> Result<(), TimerError> {
        self.api_context
            .tell(self.timer_address, t::ScheduleOnce::<T> { key, at, msg })
            .await
            .map_err(TimerError::Send)?;
        Ok(())
    }

    async fn schedule_once_after(
        &mut self,
        key: Self::Key,
        after: Self::Duration,
        msg: Self::Message,
    ) -> Result<(), TimerError> {
        let now = self.api_context.now();
        let at = T::instant_plus_duration(now, after);
        self.api_context
            .tell(self.timer_address, t::ScheduleOnce::<T> { key, at, msg })
            .await
            .map_err(TimerError::Send)?;
        Ok(())
    }
}
