use std::collections::BTreeMap;
use std::sync::Arc;

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::subnet::NetAddress;
use mm1_ask::Reply;
use mm1_common::log;
use mm1_common::types::{AnyError, Never};
use mm1_core::context::{Bind, BindArgs, Fork, InitDone, Messaging, Now, Quit};
use mm1_core::envelope::dispatch;
use mm1_proto_ask::Request;
use mm1_proto_network_management::RegisterSubnetRequest;
use mm1_timer::api::TimerApi;
use tokio::time::Instant;

use crate::network_manager::messages::Tick;

const DEFAULT_INBOX_SIZE: usize = 1024;

mod messages;
mod register_subnet;
mod subnet_controller;

pub async fn network_manager_actor<Ctx, SubnetConfig>(
    ctx: &mut Ctx,
    bind_to_networks: impl IntoIterator<Item = NetAddress>,
    remote_subnet_sup: Address,
) -> Result<Never, AnyError>
where
    Ctx: Bind<NetAddress>
        + InitDone
        + Fork
        + Messaging
        + Now<Instant = Instant>
        + Reply
        + Quit
        + Sync,
    SubnetConfig: serde::de::DeserializeOwned + Send + Sync + 'static,
{
    let mut ticks = mm1_timer::new_tokio_timer::<NetAddress, Tick, _>(ctx).await?;

    for bind_to in bind_to_networks {
        ctx.bind(BindArgs {
            bind_to,
            inbox_size: DEFAULT_INBOX_SIZE,
        })
        .await
        .inspect_err(|reason| log::error!("failed to bind to {}: {}", bind_to, reason))?;

        log::info!("bound to {}", bind_to);
    }

    ctx.init_done(ctx.address()).await;

    let mut state: State<SubnetConfig> = Default::default();

    loop {
        let envelope = ctx.recv().await?;

        dispatch!(match envelope {
            Request::<RegisterSubnetRequest> { header, payload } => {
                let response = register_subnet::process_request(&mut state, payload)
                    .await
                    .inspect_err(|failure| log::error!("network manager failure: {}", failure))?
                    .inspect_err(|error| log::warn!("registration error: {}", error));

                let _ = ctx.reply(header, response).await;
            },

            messages::Tick => {
                log::debug!("tick")
            },

            messages::SubnetStarted {
                net_address,
                handled_by,
            } => {
                subnet_controller::started(&mut state, net_address, handled_by).await?;
            },
            messages::SubnetStartFailed { net_address } => {
                subnet_controller::start_failed(&mut state, net_address).await?;
            },
        });

        for (address_range, subnet_state) in &mut state.subnets {
            let SubnetState::Remote(subnet_state) = subnet_state else {
                continue
            };
            let Some(action) = subnet_controller::decide(Instant::now(), subnet_state).await else {
                continue
            };

            match action {
                Action::Reset => {
                    subnet_state.status = SubnetStatus::Down;
                },

                Action::TickAt(tick_at) => {
                    ticks
                        .schedule_once_at((*address_range).into(), tick_at, messages::Tick)
                        .await?;
                },

                Action::Start => {
                    let () = subnet_controller::start(
                        ctx,
                        remote_subnet_sup,
                        NetAddress::from(*address_range),
                        subnet_state.config.clone(),
                    )
                    .await?;
                    subnet_state.status = SubnetStatus::StartingUp;
                },

                Action::Stop => {
                    unimplemented!()
                },
            }
        }
    }
}

struct State<C> {
    subnets: BTreeMap<AddressRange, SubnetState<C>>,
}

enum SubnetState<C> {
    Local,
    Remote(RemoteSubnetState<C>),
}

struct RemoteSubnetState<C> {
    config: Arc<C>,
    target: SubnetTarget,
    status: SubnetStatus,
}

#[derive(Debug, Clone, Copy)]
enum SubnetTarget {
    Up,

    #[allow(dead_code)]
    Down,
}

#[derive(Debug, Clone, Copy)]
enum SubnetStatus {
    Down,
    StartingUp,
    Up(#[allow(dead_code)] Address),

    #[allow(dead_code)]
    ShuttingDown,

    Failed(Instant),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    Start,
    Stop,
    Reset,
    TickAt(Instant),
}

impl<C> RemoteSubnetState<C> {
    fn new(subnet_config: C) -> Self {
        let status = SubnetStatus::Down;
        let config = Arc::new(subnet_config);
        Self {
            config,
            target: SubnetTarget::Up,
            status,
        }
    }
}

impl<C> Default for State<C> {
    fn default() -> Self {
        Self {
            subnets: Default::default(),
        }
    }
}
