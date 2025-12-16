use std::ops::ControlFlow;

use mm1_address::address::Address;
use mm1_common::types::Never;

#[derive(derive_more::Debug)]
pub struct Outcome<Rq, Rs = Never> {
    pub(crate) action: Action<Rq, Rs>,
    pub(crate) then:   ControlFlow<()>,
}

#[derive(Default, derive_more::Debug, derive_more::From)]
pub(crate) enum Action<Rq, Rs> {
    #[default]
    Nothing,
    Forward(OutcomeForward<Rq>),
    Reply(OutcomeReply<Rs>),
}

#[derive(derive_more::Debug)]
pub(crate) struct OutcomeForward<Rq> {
    pub(crate) to:      Address,
    #[debug(skip)]
    pub(crate) request: Rq,
}

#[derive(derive_more::Debug)]
pub(crate) struct OutcomeReply<Rs> {
    #[debug(skip)]
    pub(crate) response: Rs,
}

impl<Rq, Rs> Outcome<Rq, Rs> {
    pub fn forward(to: Address, request: Rq) -> Self {
        let forward = OutcomeForward { to, request };
        Self {
            action: forward.into(),
            then:   ControlFlow::Continue(()),
        }
    }

    pub fn reply(response: Rs) -> Self {
        let reply = OutcomeReply { response };
        Self {
            action: reply.into(),
            then:   ControlFlow::Continue(()),
        }
    }

    pub fn no_reply() -> Self {
        Self {
            action: Default::default(),
            then:   ControlFlow::Continue(()),
        }
    }

    pub fn then_stop(self) -> Self {
        Self {
            then: ControlFlow::Break(()),
            ..self
        }
    }
}
