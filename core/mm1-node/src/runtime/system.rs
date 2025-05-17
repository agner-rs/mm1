use mm1_address::address::Address;

use crate::runtime::context::ActorContext;
use crate::runtime::runnable;

mod protocol_system;

#[derive(Debug, Default, Clone, Copy)]
pub struct Local;

#[derive(Debug, Clone, Copy)]
pub struct Remote(#[allow(dead_code)] Address);

pub struct RemoteRunnable(Address);

impl Local {
    pub fn actor<F>(actor: F) -> <Self as mm1_proto_system::System>::Runnable
    where
        F: runnable::ActorRunBoxed<ActorContext> + 'static,
    {
        runnable::boxed_from_fn(actor)
    }
}

impl mm1_proto_system::System for Local {
    type Runnable = runnable::BoxedRunnable<ActorContext>;
}

impl mm1_proto_system::System for Remote {
    type Runnable = RemoteRunnable;
}

impl mm1_proto_system::Runnable for RemoteRunnable {
    type System = Remote;

    fn run_at(&self) -> Self::System {
        Remote(self.0)
    }
}
