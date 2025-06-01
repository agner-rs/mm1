use std::hash::Hash;
use std::marker::PhantomData;

use futures::FutureExt;
use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_core::context::{Fork, ForkErrorKind, Messaging, Now, Quit, SendErrorKind, Tell};
use mm1_core::prim::Message;
use mm1_proto_timer as t;

#[cfg(feature = "tokio-time")]
pub async fn new_tokio_timer<K, M, Ctx>(
    ctx: &mut Ctx,
) -> Result<
    crate::api::TimerApi<
        K,
        M,
        Ctx,
        impl mm1_proto_timer::Timer<
            Key = K,
            Message = M,
            Instant = tokio::time::Instant,
            Duration = std::time::Duration,
        >,
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
    Ctx::Instant: Send,
{
    TimerApi::<K, M, Ctx, crate::tokio_time::TokioTimer<K, M>>::new(ctx).await
}

#[derive(Debug, thiserror::Error)]
pub enum TimerError {
    #[error("fork: {}", _0)]
    Fork(ErrorOf<ForkErrorKind>),
    #[error("send: {}", _0)]
    Send(ErrorOf<SendErrorKind>),
}

pub struct TimerApi<Key, Msg, Ctx, T> {
    api_context:   Ctx,
    timer_address: Address,
    _pd:           PhantomData<(Key, Msg, T)>,
}

impl<Ctx, Key, Msg, T> TimerApi<Key, Msg, Ctx, T>
where
    Ctx: Quit + Fork + Messaging + Now,
    Key: Hash + Ord + Clone + Send + Sync + 'static,
    Msg: Message,
    T: t::Timer<Key = Key, Message = Msg, Instant = Ctx::Instant>,
    Ctx::Instant: Send,
{
    pub async fn new(ctx: &mut Ctx) -> Result<Self, TimerError> {
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

    pub async fn cancel(&mut self, key: T::Key) -> Result<(), TimerError> {
        self.api_context
            .tell(self.timer_address, t::Cancel::<T> { key })
            .await
            .map_err(TimerError::Send)?;
        Ok(())
    }

    pub async fn schedule_once_at(
        &mut self,
        key: Key,
        at: T::Instant,
        msg: Msg,
    ) -> Result<(), TimerError> {
        self.api_context
            .tell(self.timer_address, t::ScheduleOnce::<T> { key, at, msg })
            .await
            .map_err(TimerError::Send)?;
        Ok(())
    }

    pub async fn schedule_once_after(
        &mut self,
        key: Key,
        after: T::Duration,
        msg: Msg,
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
