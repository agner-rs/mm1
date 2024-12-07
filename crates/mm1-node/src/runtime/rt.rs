use tracing::{instrument, warn};

use crate::runtime::actor_key::ActorKey;
use crate::runtime::config::{
    RtConfig, {self},
};
use crate::runtime::container::{Container, ContainerArgs, ContainerError};
use crate::runtime::context;
use crate::runtime::rt_api::{RequestAddressError, RtApi};
use crate::runtime::runnable::BoxedRunnable;

#[derive(Debug)]
pub struct Rt {
    #[allow(unused)]
    config: RtConfig,
    rt_api: RtApi,
}

#[derive(Debug, thiserror::Error)]
pub enum RtCreateError {}

#[derive(Debug, thiserror::Error)]
pub enum RtRunError {
    #[error("request address error: {}", _0)]
    RequestAddressError(
        #[allow(private_interfaces)]
        #[source]
        RequestAddressError,
    ),
    #[error("container error: {}", _0)]
    #[allow(private_interfaces)]
    ContainerError(#[source] ContainerError),
}

impl Rt {
    pub fn create(config: RtConfig) -> Result<Self, RtCreateError> {
        let rt_api = RtApi::create(config.subnet_address);

        Ok(Self { config, rt_api })
    }

    #[instrument(skip_all, fields(func = main_actor.func_name()))]
    pub async fn run(
        self,
        main_actor: BoxedRunnable<context::ActorContext>,
    ) -> Result<(), RtRunError> {
        let Self { config: _, rt_api } = self;
        let subnet_lease = rt_api
            .request_address(config::stubs::ACTOR_NETMASK)
            .await
            .map_err(RtRunError::RequestAddressError)?;
        let args = ContainerArgs {
            ack_to: None,
            link_to: Default::default(),
            actor_key: ActorKey::root().child(main_actor.func_name(), Default::default()),
            inbox_size: config::stubs::INBOX_SIZE,
            subnet_lease,
            rt_api: rt_api.clone(),
        };
        let (_subnet_lease, exit_reason) = Container::create(args, main_actor)
            .map_err(RtRunError::ContainerError)?
            .run()
            .await
            .map_err(RtRunError::ContainerError)?;

        if let Err(failure) = exit_reason {
            warn!("main-actor failure: {}", failure);
        }

        Ok(())
    }
}
