use std::collections::HashMap;
use std::sync::Arc;

use mm1_address::address::Address;
use mm1_common::errors::chain::ExactTypeDisplayChainExt;
use mm1_common::types::AnyError;
use mm1_core::tracing::TraceId;
use mm1_runnable::local::{self, BoxedRunnable};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc;
use tracing::{error, instrument, trace};

use crate::actor_key::ActorKey;
use crate::config::{EffectiveActorConfig, Mm1NodeConfig, Valid};
use crate::init::InitActorArgs;
use crate::runtime::container::{Container, ContainerArgs, ContainerError};
use crate::runtime::context;
use crate::runtime::rt_api::{RequestAddressError, RtApi};

#[derive(Debug)]
pub struct Rt {
    #[allow(unused)]
    config:           Valid<Mm1NodeConfig>,
    rt_default:       Runtime,
    rt_named:         HashMap<String, Runtime>,
    tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
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

        let (tx_actor_failure, _rx_actor_failure) = mpsc::unbounded_channel();
        Ok(Self {
            config,
            rt_default,
            rt_named,
            tx_actor_failure,
        })
    }

    pub fn with_actor_failure_sink(
        self,
        tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
    ) -> Self {
        Self {
            tx_actor_failure,
            ..self
        }
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
            local_subnet_auto: config.local_subnet_address_auto(),
            local_subnets_bind: config.local_subnet_addresses_bind().collect(),
            #[cfg(feature = "multinode")]
            multinode_inbound: config.multinode_inbound().collect(),
            #[cfg(feature = "multinode")]
            multinode_outbound: config.multinode_outbound().collect(),
        };

        rt_handle.block_on(run_inner(
            config.clone(),
            ActorKey::root(),
            crate::init::init_actor_config(),
            rt_default,
            rt_named,
            local::boxed_from_fn((crate::init::run, (main_actor, init_actor_args))),
            self.tx_actor_failure.clone(),
        ))
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
    tx_actor_failure: mpsc::UnboundedSender<(Address, AnyError)>,
) -> Result<(), RtRunError> {
    let rt_api = RtApi::create(config.local_subnet_address_auto(), rt_default, rt_named);

    let subnet_lease = rt_api
        .request_address(actor_config.netmask())
        .await
        .map_err(RtRunError::RequestAddressError)?;

    let args = ContainerArgs {
        ack_to: None,
        link_to: Default::default(),
        actor_key,
        trace_id: TraceId::random(),
        subnet_lease,
        rt_api: rt_api.clone(),
        rt_config: Arc::new(config),
    };
    trace!("creating and running container...");
    let exit_reason = Container::create(args, main_actor, tx_actor_failure)
        .map_err(RtRunError::ContainerError)?
        .run()
        .await
        .map_err(RtRunError::ContainerError)?;

    trace!(reason = ?exit_reason.as_ref().map_err(|e| e.as_display_chain()), "container exited");

    if let Err(failure) = exit_reason {
        error!(
            error = %failure.as_display_chain(),
            "main-actor failure"
        );
    }

    Ok(())
}
