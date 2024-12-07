use std::future::Future;

use mm1_address::address::Address;
use mm1_proto_system as system;
use mm1_proto_system::System;

use crate::context::call::Call;

pub trait Linking<Sys>:
    Call<Sys, system::Link, Outcome = ()>
    + Call<Sys, system::Unlink, Outcome = ()>
    + Call<Sys, system::TrapExit, Outcome = ()>
where
    Sys: System,
{
    fn link(&mut self, peer: Address) -> impl Future<Output = ()> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::Link { peer }).await }
    }

    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::Unlink { peer }).await }
    }

    fn set_trap_exit(&mut self, enable: bool) -> impl Future<Output = ()> + Send
    where
        Sys: Default,
    {
        async move { self.call(Sys::default(), system::TrapExit { enable }).await }
    }
}

impl<Sys, T> Linking<Sys> for T
where
    T: Call<Sys, system::Link, Outcome = ()>
        + Call<Sys, system::Unlink, Outcome = ()>
        + Call<Sys, system::TrapExit, Outcome = ()>,
    Sys: System,
{
}
