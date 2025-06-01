use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::{Handle, Runtime};
use tracing::{instrument, warn};

use super::config::EffectiveActorConfig;
use crate::runtime::actor_key::ActorKey;
use crate::runtime::config::Mm1Config;
use crate::runtime::container::{Container, ContainerArgs, ContainerError};
use crate::runtime::context;
use crate::runtime::rt_api::{RequestAddressError, RtApi};
use crate::runtime::runnable::BoxedRunnable;

#[derive(Debug)]
pub struct Rt {
    #[allow(unused)]
    config: Mm1Config,

    rt_default: Runtime,
    rt_named:   HashMap<String, Runtime>,
}

#[derive(Debug, thiserror::Error)]
pub enum RtCreateError {
    #[error("runtime config error: {}", _0)]
    RuntimeConfigError(String),
    #[error("runtime init error: {}", _0)]
    RuntimeInitError(#[source] std::io::Error),
}

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
    pub fn create(config: Mm1Config) -> Result<Self, RtCreateError> {
        config
            .validate()
            .map_err(RtCreateError::RuntimeConfigError)?;
        let (rt_default, rt_named) = config
            .build_runtimes()
            .map_err(RtCreateError::RuntimeInitError)?;

        Ok(Self {
            config,
            rt_default,
            rt_named,
        })
    }

    pub fn run(&self, main_actor: BoxedRunnable<context::ActorContext>) -> Result<(), RtRunError> {
        let config = self.config.clone();
        let rt_default = self.rt_default.handle().to_owned();
        let rt_named = self
            .rt_named
            .iter()
            .map(|(k, v)| (k.to_owned(), v.handle().to_owned()))
            .collect::<HashMap<_, _>>();

        let main_actor_key = ActorKey::root().child(main_actor.func_name());
        let main_actor_config = config.actor_config(&main_actor_key);
        let rt_handle = if let Some(rt_key) = main_actor_config.runtime_key() {
            rt_named
                .get(rt_key)
                .cloned()
                .expect("the config's validity should have been checked")
        } else {
            rt_default.clone()
        };

        rt_handle.block_on(run_inner(
            config.clone(),
            main_actor_key,
            main_actor_config,
            rt_default,
            rt_named,
            main_actor,
        ))
    }
}

#[instrument(skip_all, fields(func = main_actor.func_name()))]
async fn run_inner(
    config: Mm1Config,
    actor_key: ActorKey,
    actor_config: impl EffectiveActorConfig,
    rt_default: Handle,
    rt_named: HashMap<String, Handle>,
    main_actor: BoxedRunnable<context::ActorContext>,
) -> Result<(), RtRunError> {
    let rt_api = RtApi::create(config.subnet, rt_default, rt_named);

    let subnet_lease = rt_api
        .request_address(actor_config.netmask())
        .await
        .map_err(RtRunError::RequestAddressError)?;

    let args = ContainerArgs {
        ack_to: None,
        link_to: Default::default(),
        actor_key,

        subnet_lease,
        rt_api: rt_api.clone(),
        rt_config: Arc::new(config),
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
