#[cfg(feature = "multinode")]
use std::time::Duration;

use eyre::Context;
use futures::never::Never;
use mm1_address::subnet::{NetAddress, NetMask};
use mm1_common::log;
use mm1_common::types::AnyError;
use mm1_core::context::{Linking, Messaging, Quit, Start};
use mm1_core::envelope::dispatch;
#[cfg(feature = "multinode")]
use mm1_proto_network_management as nm;
use mm1_runnable::local::BoxedRunnable;

#[cfg(feature = "multinode")]
use crate::config::DefAddr;
use crate::config::EffectiveActorConfig;
use crate::runtime::ActorContext;

#[cfg(feature = "multinode")]
const MULTINODE_MANAGER_ASK_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct InitActorArgs {
    pub(crate) local_subnet_auto:  NetAddress,
    pub(crate) local_subnets_bind: Vec<NetAddress>,
    #[cfg(feature = "multinode")]
    pub(crate) multinode_inbound:  Vec<(Vec<nm::ProtocolName>, DefAddr, nm::Options)>,
    #[cfg(feature = "multinode")]
    pub(crate) multinode_outbound: Vec<(Vec<nm::ProtocolName>, DefAddr, nm::Options)>,
}

pub(crate) async fn run(
    ctx: &mut ActorContext,
    main_actor: BoxedRunnable<ActorContext>,
    args: InitActorArgs,
) -> Result<Never, AnyError> {
    ctx.set_trap_exit(true).await;

    let InitActorArgs {
        #[allow(unused)]
        local_subnet_auto,
        #[allow(unused)]
        local_subnets_bind,

        #[cfg(feature = "multinode")]
        multinode_outbound,
        #[cfg(feature = "multinode")]
        multinode_inbound,
    } = args;

    #[cfg(feature = "name-service")]
    {
        use std::time::Duration;

        use eyre::Context;
        use mm1_runnable::local;

        let name_service_address = ctx
            .start(
                local::boxed_from_fn((
                    mm1_name_service::server::name_server_actor,
                    ([mm1_proto_well_known::NAME_SERVICE.into()],),
                )),
                true,
                Duration::from_secs(1),
            )
            .await
            .wrap_err("name-service start")?;
        log::debug!(started = %name_service_address, "started name-service");
    };

    #[cfg(feature = "multinode")]
    {
        use std::time::Duration;

        use eyre::Context;
        use mm1_ask::Ask;
        use mm1_common::log::info;
        use mm1_multinode::actors::multinode_manager;
        use mm1_runnable::local;

        let multinode_manager_address = ctx
            .start(
                local::boxed_from_fn(multinode_manager::run),
                true,
                Duration::from_secs(1),
            )
            .await
            .wrap_err("multinode-connection-manager start")?;
        log::debug!(
            started = %multinode_manager_address,
            "started multinode-connection-manager"
        );

        for net in local_subnets_bind.into_iter().chain([local_subnet_auto]) {
            use mm1_proto_network_management::protocols;

            info!(%net, "registering local-subnet");

            type Ret = protocols::RegisterLocalSubnetResponse;
            let request = protocols::RegisterLocalSubnetRequest { net };
            let () = ctx
                .ask::<_, Ret>(
                    multinode_manager_address,
                    request,
                    MULTINODE_MANAGER_ASK_TIMEOUT,
                )
                .await
                .wrap_err("ctx.ask::<nm::RegisterLocalSubnet>")?
                .wrap_err("nm::RegisterLocalSubnet")?;
        }

        for (protocol_names, bind_address, options) in multinode_inbound {
            use mm1_proto_network_management::iface;

            info!(
                %bind_address, ?protocol_names,
                "adding inbound multinode-interface"
            );

            type Ret = iface::BindResponse;
            let () = match bind_address {
                DefAddr::Tcp(bind_address) => {
                    let request = iface::BindRequest {
                        protocol_names,
                        bind_address,
                        options,
                    };
                    ctx.ask::<_, Ret>(
                        multinode_manager_address,
                        request,
                        MULTINODE_MANAGER_ASK_TIMEOUT,
                    )
                    .await
                },
                DefAddr::Uds(bind_address) => {
                    let request = iface::BindRequest {
                        protocol_names,
                        bind_address,
                        options,
                    };
                    ctx.ask::<_, Ret>(
                        multinode_manager_address,
                        request,
                        MULTINODE_MANAGER_ASK_TIMEOUT,
                    )
                    .await
                },
            }
            .wrap_err("ctx.ask::<nm::Bind>")?
            .wrap_err("nm::Bind")?;
        }

        for (protocol_name, dst_address, options) in multinode_outbound {
            use mm1_proto_network_management::iface;

            info!(
                ?protocol_name, %dst_address,
                "adding outbound multinode-interface"
            );

            type Ret = iface::ConnectResponse;
            let () = match dst_address {
                DefAddr::Tcp(dst_address) => {
                    let request = iface::ConnectRequest {
                        protocol_names: protocol_name,
                        dst_address,
                        options,
                    };
                    ctx.ask::<_, Ret>(
                        multinode_manager_address,
                        request,
                        MULTINODE_MANAGER_ASK_TIMEOUT,
                    )
                    .await
                },
                DefAddr::Uds(dst_address) => {
                    let request = iface::ConnectRequest {
                        protocol_names: protocol_name,
                        dst_address,
                        options,
                    };
                    ctx.ask::<_, Ret>(
                        multinode_manager_address,
                        request,
                        MULTINODE_MANAGER_ASK_TIMEOUT,
                    )
                    .await
                },
            }
            .wrap_err("ctx.ask::<nm::Connect>")?
            .wrap_err("nm::Connect")?;
        }
    }

    log::debug!(main_actor_func = %main_actor.func_name(), "about to start main-actor");
    let main_actor_address = ctx
        .spawn(main_actor, true)
        .await
        .wrap_err("main-actor spawn")?;
    log::debug!(started = %main_actor_address, "started main-actor");

    let main_actor_exited = loop {
        let envelope = ctx.recv().await.wrap_err("init-actor recv")?;
        dispatch!(match envelope {
            exited @ mm1_proto_system::Exited { .. } => break exited,
            unexpected @ _ => {
                log::warn!(
                    msg = ?unexpected,
                    "init-actor received an unexpected message"
                );
            },
        })
    };

    log::info!(exited = ?main_actor_exited, "main-actor exited");

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

    fn message_tap_key(&self) -> Option<&str> {
        None
    }
}
