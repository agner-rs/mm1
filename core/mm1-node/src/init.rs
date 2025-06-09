use futures::never::Never;
use mm1_address::subnet::NetMask;
use mm1_common::log;
use mm1_common::types::AnyError;
use mm1_core::context::{Linking, Messaging, Quit, Start};
use mm1_core::envelope::dispatch;
use mm1_runnable::local::BoxedRunnable;

use crate::config::EffectiveActorConfig;
use crate::runtime::ActorContext;

pub(crate) struct InitActorArgs {
    #[cfg(feature = "multinode")]
    pub(crate) codec_registry: mm1_multinode::codecs::CodecRegistry,
}

pub(crate) async fn run(
    ctx: &mut ActorContext,
    main_actor: BoxedRunnable<ActorContext>,
    args: InitActorArgs,
) -> Result<Never, AnyError> {
    ctx.set_trap_exit(true).await;

    let InitActorArgs {
        #[cfg(feature = "multinode")]
        codec_registry,
    } = args;

    #[cfg(feature = "name-service")]
    {
        use std::time::Duration;

        use mm1_runnable::local;

        let name_service_address = ctx
            .start(
                local::boxed_from_fn((
                    mm1_name_service::server::name_server_actor,
                    ([mm1_proto_well_known::NAME_SERVICE.into()],),
                )),
                true,
                Duration::from_millis(10),
            )
            .await?;
        log::debug!("started name-service at {}", name_service_address);
    };

    #[cfg(feature = "multinode")]
    {
        use std::time::Duration;

        use mm1_runnable::local;

        let remote_subnet_sup = ctx
            .start(
                local::boxed_from_fn((mm1_multinode::remote_subnet::sup::run, (codec_registry,))),
                true,
                Duration::from_millis(10),
            )
            .await?;

        log::debug!("started remote-subnet-sup at {}", remote_subnet_sup);

        let network_manager_address = ctx
            .start(
                local::boxed_from_fn((
                    mm1_multinode::network_manager::network_manager_actor::<
                        _,
                        mm1_multinode::remote_subnet::config::RemoteSubnetConfig,
                    >,
                    (
                        [mm1_proto_well_known::NETWORK_MANAGER.into()],
                        remote_subnet_sup,
                    ),
                )),
                true,
                Duration::from_millis(10),
            )
            .await?;
        log::debug!("started network-manager at {}", network_manager_address);
    };

    #[cfg(feature = "multinode")]
    {
        use std::time::Duration;

        use mm1_ask::Ask;
        use mm1_core::context::Fork;
        use mm1_proto_network_management::{RegisterSubnetRequest, RegisterSubnetResponse};
        use serde_json::json;

        use crate::config::SubnetKind;

        let mut ctx_register_subnet = ctx.fork().await?;
        for subnet in ctx.rt_config.subnets() {
            let config = match &subnet.kind {
                SubnetKind::Local => json!({"type": "local"}),
                SubnetKind::Remote(remote) => {
                    json!({
                        "type": "remote",
                        "props": remote,
                    })
                },
            };

            let request = RegisterSubnetRequest {
                net_address: subnet.net_address,
                config,
            };

            let response: RegisterSubnetResponse = ctx_register_subnet
                .ask(
                    mm1_proto_well_known::NETWORK_MANAGER,
                    request,
                    Duration::from_millis(100),
                )
                .await
                .inspect_err(|ask_error| {
                    log::error!(
                        "error registering {}; asking network-manager failed: {}",
                        subnet.net_address,
                        ask_error
                    )
                })?;
            let () = response.inspect_err(|reason| {
                log::error!(
                    "error registering {}; network-manager replied: {}",
                    subnet.net_address,
                    reason
                )
            })?;

            log::debug!("registered {}", subnet.net_address);
        }
    }

    log::debug!("about to start main-actor: {}", main_actor.func_name());
    let main_actor_address = ctx.spawn(main_actor, true).await?;
    log::debug!("main-actor address: {}", main_actor_address);

    let main_actor_exited = loop {
        let envelope = ctx.recv().await?;
        dispatch!(match envelope {
            exited @ mm1_proto_system::Exited { .. } => break exited,
            unexpected @ _ => {
                log::warn!(
                    "init-actor received an unexpected message: {:?}",
                    unexpected
                );
            },
        })
    };

    log::info!("main-actor exited: {:?}", main_actor_exited);

    Ok(ctx.quit_ok().await)
}

pub(crate) fn init_actor_config() -> impl EffectiveActorConfig {
    InitActorConfig
}

struct InitActorConfig;

impl EffectiveActorConfig for InitActorConfig {
    fn inbox_size(&self) -> usize {
        1
    }

    fn netmask(&self) -> NetMask {
        NetMask::M_60
    }

    fn runtime_key(&self) -> Option<&str> {
        None
    }
}
