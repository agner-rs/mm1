fn main() {}

#[cfg(feature = "tokio-time")]
#[allow(dead_code)]
mod with_tokio_time {
    use std::time::Duration;

    use mm1_core::context::{Fork, Messaging, Now, Quit};

    async fn main_actor<Ctx>(ctx: &mut Ctx)
    where
        Ctx: Quit + Messaging + Fork + Now<Instant = tokio::time::Instant>,
    {
        let mut timers = mm1_timer::new_tokio_timer::<&'static str, i32, _>(ctx)
            .await
            .unwrap();
        timers
            .schedule_once_at(
                "one",
                tokio::time::Instant::now()
                    .checked_add(Duration::from_secs(1))
                    .unwrap(),
                1,
            )
            .await
            .unwrap();
        timers
            .schedule_once_after("two", Duration::from_secs(2), 2)
            .await
            .unwrap();

        timers.cancel("one").await.unwrap();
    }
}
