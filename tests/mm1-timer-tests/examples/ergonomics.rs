fn main() {}

use std::time::Duration;

use mm1::core::context::{Fork, Messaging, Now, Quit};
use mm1::timer::v1::OneshotTimer;

#[allow(unused)]
async fn main_actor<Ctx>(ctx: &mut Ctx)
where
    Ctx: Quit + Messaging + Fork + Now<Instant = tokio::time::Instant>,
{
    let mut timers = OneshotTimer::create(ctx).await.unwrap();
    let one = timers
        .schedule_once_at(
            tokio::time::Instant::now()
                .checked_add(Duration::from_secs(1))
                .unwrap(),
            1,
        )
        .await
        .unwrap();
    let _two = timers
        .schedule_once_after(Duration::from_secs(2), 2)
        .await
        .unwrap();

    timers.cancel(one).await.unwrap();
}
