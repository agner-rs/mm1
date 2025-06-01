use std::collections::{BTreeSet, HashMap};
use std::ops::Sub;

use mm1_address::address::Address;
use mm1_common::log;
use mm1_common::types::Never;
use mm1_core::context::{Messaging, Now, Quit, Tell};
use mm1_core::envelope::{Envelope, dispatch};
use mm1_proto_timer as t;
use mm1_proto_timer::Timer;
use tokio::task;

pub async fn timer_actor<T, Ctx>(ctx: &mut Ctx, receiver: Address) -> Never
where
    T: Timer,
    Ctx: Messaging + Now<Instant = T::Instant> + Quit,
{
    match inner::<T, _>(ctx, receiver).await {
        Ok(never) => never,
        Err(reason) => ctx.quit_err(Error(reason)).await,
    }
}
#[derive(Debug, thiserror::Error)]
#[error("{}", _0)]
struct Error(#[source] Box<dyn std::error::Error + Send + Sync + 'static>);

async fn inner<T, Ctx>(
    ctx: &mut Ctx,
    receiver: Address,
) -> Result<Never, Box<dyn std::error::Error + Send + Sync + 'static>>
where
    T: Timer,
    Ctx: Messaging + Now<Instant = T::Instant> + Quit,
{
    let mut state: State<T> = Default::default();
    loop {
        let now = ctx.now();
        let next_at = state.next_at();
        let wake_fut = async move {
            if let Some(at) = next_at {
                let dt = checked_sub(at, now).unwrap_or_default();
                T::sleep(dt).await
            } else {
                std::future::pending().await
            }
        };
        let recv_fut = ctx.recv();

        enum Selected {
            WakeUp,
            Envelope(Envelope),
        }
        let selected = tokio::select! {
            () = wake_fut => {
                log::trace!("wake");
                Selected::WakeUp
            },
            recv_result = recv_fut => {
                log::trace!("recv");
                let envelope = recv_result?;
                Selected::Envelope(envelope)
            }
        };

        match selected {
            Selected::Envelope(envelope) => {
                dispatch!(match envelope {
                    t::ScheduleOnce::<T> { key, at, msg } => {
                        log::trace!("recv:schedule");
                        state.schedule_once(key, at, msg)
                    },
                    t::Cancel::<T> { key } => {
                        log::trace!("recv:cancel");
                        state.cancel(key)
                    },
                })
            },
            Selected::WakeUp => {
                let now = ctx.now();
                while let Some(msg) = state.take_elapsed(now) {
                    log::trace!("wake:elapsed");

                    // FIXME: should probably, in case of `Err(Full)`, yield and retry several times
                    ctx.tell(receiver, msg).await?;

                    task::yield_now().await;
                }
                log::trace!("wake:done");
            },
        }
    }
}

struct State<T: Timer> {
    upcoming: BTreeSet<(T::Instant, T::Key)>,
    entries:  HashMap<T::Key, Entry<T::Instant, T::Message>>,
}
struct Entry<I, M> {
    at:      I,
    msg_gen: MsgGen<M>,
}

enum MsgGen<M> {
    Once(M),
}

impl<T: Timer> Default for State<T> {
    fn default() -> Self {
        Self {
            upcoming: Default::default(),
            entries:  Default::default(),
        }
    }
}

impl<T: Timer> State<T> {
    fn schedule_once(&mut self, key: T::Key, at: T::Instant, message: T::Message) {
        if let Some(existing_entry) = self.entries.insert(
            key.clone(),
            Entry {
                at,
                msg_gen: MsgGen::Once(message),
            },
        ) {
            let former_at = existing_entry.at;
            let existed_before = self.upcoming.remove(&(former_at, key.clone()));

            assert!(existed_before);
        }

        let newly_inserted = self.upcoming.insert((at, key));

        assert!(newly_inserted);
    }

    fn cancel(&mut self, key: T::Key) {
        let Some(existing_entry) = self.entries.remove(&key) else {
            return
        };
        let Entry { at, msg_gen: _ } = existing_entry;
        let existed = self.upcoming.remove(&(at, key));
        assert!(existed);
    }

    fn next_at(&self) -> Option<T::Instant> {
        self.upcoming.first().map(|(t, _k)| *t)
    }

    fn take_elapsed(&mut self, now: T::Instant) -> Option<T::Message> {
        let _ = self.upcoming.first().filter(|(t, _)| *t <= now)?;

        let (_at, key) = self
            .upcoming
            .pop_first()
            .expect(".first() returned Some. We expect .pop_first() to return Some as well");

        let Entry { at: _, msg_gen } = self
            .entries
            .remove(&key)
            .expect("`upcoming` contains a key that does not exist in `entries`");

        match msg_gen {
            MsgGen::Once(message) => Some(message),
        }
    }
}

fn checked_sub<I, D>(l: I, r: I) -> Option<D>
where
    I: Ord + Sub<I, Output = D>,
{
    r.cmp(&l).is_ge().then(|| r.sub(l))
}
