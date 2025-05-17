use std::future::Future;

use mm1_address::address::Address;
use mm1_proto_system as system;
use mm1_proto_system::System;

use crate::context::call::Call;

pub trait Watching<Sys>:
    Call<Sys, system::Watch, Outcome = system::WatchRef> + Call<Sys, system::Unwatch, Outcome = ()>
where
    Sys: System,
{
    fn watch(&mut self, peer: Address) -> impl Future<Output = system::WatchRef> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::Watch { peer }).await }
    }

    fn unwatch(&mut self, wait_ref: system::WatchRef) -> impl Future<Output = ()> + Send
    where
        Sys: Default,
    {
        async move {
            self.call(
                Sys::default(),
                system::Unwatch {
                    watch_ref: wait_ref,
                },
            )
            .await
        }
    }
}

impl<Sys, T> Watching<Sys> for T
where
    T: Call<Sys, system::Watch, Outcome = system::WatchRef>
        + Call<Sys, system::Unwatch, Outcome = ()>,
    Sys: System,
{
}
