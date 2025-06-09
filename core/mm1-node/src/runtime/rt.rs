use std::collections::HashMap;
use std::sync::Arc;

use mm1_runnable::local::{self, BoxedRunnable};
use tokio::runtime::{Handle, Runtime};
use tracing::{instrument, warn};

use crate::actor_key::ActorKey;
use crate::config::{EffectiveActorConfig, Mm1NodeConfig, Valid};
use crate::init::InitActorArgs;
use crate::runtime::container::{Container, ContainerArgs, ContainerError};
use crate::runtime::context;
use crate::runtime::rt_api::{RequestAddressError, RtApi};

#[derive(Debug)]
pub struct Rt {
    #[allow(unused)]
    config:     Valid<Mm1NodeConfig>,
    rt_default: Runtime,
    rt_named:   HashMap<String, Runtime>,

    #[cfg(feature = "multinode")]
    multinode_codecs: mm1_multinode::codecs::CodecRegistry,
}

#[derive(Debug, thiserror::Error)]
pub enum RtCreateError {
    #[error("runtime config error: {}", _0)]
    RuntimeConfigError(crate::config::ValidationError<Mm1NodeConfig>),
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
    pub fn create(config: Mm1NodeConfig) -> Result<Self, RtCreateError> {
        let config = config
            .validate()
            .map_err(RtCreateError::RuntimeConfigError)?;
        let (rt_default, rt_named) = config
            .build_runtimes()
            .map_err(RtCreateError::RuntimeInitError)?;

        Ok(Self {
            config,
            rt_default,
            rt_named,

            #[cfg(feature = "multinode")]
            multinode_codecs: Default::default(),
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

        let rt_handle = rt_default.clone();

        let init_actor_args = InitActorArgs {
            #[cfg(feature = "multinode")]
            codec_registry:                               self.multinode_codecs.clone(),
        };

        rt_handle.block_on(run_inner(
            config.clone(),
            ActorKey::root(),
            crate::init::init_actor_config(),
            rt_default,
            rt_named,
            local::boxed_from_fn((crate::init::run, (main_actor, init_actor_args))),
        ))
    }
}

#[cfg(feature = "multinode")]
impl Rt {
    pub fn add_codec(
        &mut self,
        codec_name: &str,
        codec: mm1_multinode::codecs::Codec,
    ) -> &mut Self {
        self.multinode_codecs.add_codec(codec_name, codec);
        self
    }

    pub fn with_codec(mut self, codec_name: &str, codec: mm1_multinode::codecs::Codec) -> Self {
        self.add_codec(codec_name, codec);
        self
    }
}

#[instrument(skip_all, fields(func = main_actor.func_name()))]
async fn run_inner(
    config: Valid<Mm1NodeConfig>,
    actor_key: ActorKey,
    actor_config: impl EffectiveActorConfig,
    rt_default: Handle,
    rt_named: HashMap<String, Handle>,
    main_actor: BoxedRunnable<context::ActorContext>,
) -> Result<(), RtRunError> {
    let rt_api = RtApi::create(config.local_subnet_address(), rt_default, rt_named);

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
    let exit_reason = Container::create(args, main_actor)
        .map_err(RtRunError::ContainerError)?
        .run()
        .await
        .map_err(RtRunError::ContainerError)?;

    if let Err(failure) = exit_reason {
        warn!("main-actor failure: {}", failure);
    }

    Ok(())
}
