use std::sync::Arc;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_ask::Ask;
use mm1_common::log;
use mm1_common::types::AnyError;
use mm1_core::context::{Fork, Tell};
use mm1_core::prim::Message;
use mm1_proto_sup::uniform::{StartRequest, StartResponse};
use tokio::time::Instant;

use crate::network_manager::{
    Action, RemoteSubnetState, State, SubnetState, SubnetStatus, SubnetTarget, messages as m,
};

const SUP_START_ASK_TIMEOUT: Duration = Duration::from_millis(100);
const FAILING_SUBNET_RESTART_COOLDOWN: Duration = Duration::from_secs(1);

pub(super) async fn decide<C>(now: Instant, subnet_state: &RemoteSubnetState<C>) -> Option<Action> {
    match (subnet_state.status, subnet_state.target) {
        (SubnetStatus::Down, SubnetTarget::Down) => None,
        (SubnetStatus::ShuttingDown, _) => None,
        (SubnetStatus::Up(_), SubnetTarget::Up) => None,
        (SubnetStatus::StartingUp, _) => None,
        (SubnetStatus::Down, SubnetTarget::Up) => Some(Action::Start),
        (SubnetStatus::Up(_), SubnetTarget::Down) => Some(Action::Stop),
        (SubnetStatus::Failed(failed_at), target) => {
            let reset_at = failed_at
                .checked_add(FAILING_SUBNET_RESTART_COOLDOWN)
                .unwrap_or(now);
            let cooling_down = !reset_at.saturating_duration_since(now).is_zero();
            match (target, cooling_down) {
                (_, true) => Some(Action::TickAt(reset_at)),
                (SubnetTarget::Down, false) => Some(Action::Reset),
                (SubnetTarget::Up, false) => Some(Action::Start),
            }
        },
    }
}

pub(super) async fn start<Ctx, C>(
    ctx: &mut Ctx,
    sup: Address,
    net_address: NetAddress,
    config: Arc<C>,
) -> Result<(), AnyError>
where
    Ctx: Ask + Fork + Tell,
    C: Send + Sync,
    StartRequest<(mm1_address::subnet::NetAddress, Arc<C>)>: Message,
{
    let network_manager = ctx.address();
    let fork = ctx.fork().await?;
    fork.run(async move |mut ctx| {
        match ctx
            .ask::<_, StartResponse>(
                sup,
                StartRequest {
                    args: (net_address, config),
                },
                SUP_START_ASK_TIMEOUT,
            )
            .await
        {
            Err(ask_error) => {
                log::error!(
                    "Could not start subnet {}. Ask error: {}",
                    net_address,
                    ask_error
                );
                let _ = ctx
                    .tell(network_manager, m::SubnetStartFailed { net_address })
                    .await;
            },
            Ok(Err(sup_error)) => {
                log::error!(
                    "Could not start subnet {}. Sup error: {}",
                    net_address,
                    sup_error
                );
                let _ = ctx
                    .tell(network_manager, m::SubnetStartFailed { net_address })
                    .await;
            },
            Ok(Ok(handled_by)) => {
                log::debug!("Started subnet {} at {}", net_address, handled_by);
                let _ = ctx
                    .tell(
                        network_manager,
                        m::SubnetStarted {
                            net_address,
                            handled_by,
                        },
                    )
                    .await;
            },
        };
    })
    .await;
    Ok(())
}

pub(super) async fn started<C>(
    state: &mut State<C>,
    net_address: NetAddress,
    handled_by: Address,
) -> Result<(), AnyError> {
    let subnet_state = state
        .subnets
        .get_mut(&net_address.into())
        .ok_or_else(|| format!("no subnet_state for {}", net_address))?;
    let SubnetState::Remote(subnet_state) = subnet_state else {
        return Err(format!(
            "received a 'subnet-started' report for the local subnet {}",
            net_address
        )
        .into())
    };

    subnet_state.status = SubnetStatus::Up(handled_by);
    log::debug!("subnet {} is now handled by {}", net_address, handled_by);

    Ok(())
}

pub(super) async fn start_failed<C>(
    state: &mut State<C>,
    net_address: NetAddress,
) -> Result<(), AnyError> {
    let subnet_state = state
        .subnets
        .get_mut(&net_address.into())
        .ok_or_else(|| format!("no subnet_state for {}", net_address))?;
    let SubnetState::Remote(subnet_state) = subnet_state else {
        return Err(format!(
            "received a 'subnet-started' report for the local subnet {}",
            net_address
        )
        .into())
    };

    subnet_state.status = SubnetStatus::Failed(Instant::now());
    log::debug!("subnet {} has failed. Will restart later", net_address);

    Ok(())
}
