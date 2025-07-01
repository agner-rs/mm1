use std::sync::Arc;
use std::time::Duration;

use mm1_address::subnet::NetAddress;
use mm1_common::log;
use mm1_core::context::Bind;
use mm1_runnable::local::{self, BoxedRunnable};
use mm1_sup::common::child_spec::{ChildSpec, ChildType, InitType};
use mm1_sup::common::factory::ActorFactoryMut;
use mm1_sup::common::restart_intensity::RestartIntensity;
use mm1_sup::mixed::strategy::OneForOne;
use mm1_sup::mixed::{MixedSup, MixedSupContext, MixedSupError, mixed_sup};
use mm1_sup::uniform::{UniformSup, UniformSupContext, UniformSupFailure};

use crate::codecs::CodecRegistry;
use crate::remote_subnet::config::RemoteSubnetConfig;
use crate::remote_subnet::subnet;

const CHILD_START_TIMEOUT: Duration = Duration::from_millis(10);
const CHILD_STOP_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run<Ctx>(ctx: &mut Ctx, codec_registry: CodecRegistry) -> Result<(), UniformSupFailure>
where
    Ctx: Sync,
    Ctx: UniformSupContext<BoxedRunnable<Ctx>>,
    Ctx: MixedSupContext<local::BoxedRunnable<Ctx>>,
    Ctx: Bind<NetAddress>,
{
    log::debug!("starting remote-subnet-sup");

    let codec_registry = Arc::new(codec_registry);

    let launcher = ActorFactoryMut::new(
        move |(net_address, remote_subnet_config): (NetAddress, Arc<RemoteSubnetConfig>)| {
            local::boxed_from_fn((
                remote_subnet_with_restart,
                (codec_registry.clone(), net_address, remote_subnet_config),
            ))
        },
    );

    let child_spec = ChildSpec {
        launcher,
        child_type: (),
        init_type: InitType::WithAck {
            start_timeout: CHILD_START_TIMEOUT,
        },
        stop_timeout: CHILD_STOP_TIMEOUT,
    };
    let sup_spec = UniformSup::new(child_spec);

    mm1_sup::uniform::uniform_sup(ctx, sup_spec).await
}

pub async fn remote_subnet_with_restart<Ctx>(
    ctx: &mut Ctx,
    codec_registry: Arc<CodecRegistry>,
    net_address: NetAddress,
    remote_subnet_config: Arc<RemoteSubnetConfig>,
) -> Result<(), MixedSupError>
where
    Ctx: MixedSupContext<local::BoxedRunnable<Ctx>>,
    Ctx: Sync + Bind<NetAddress>,
{
    let launcher = ActorFactoryMut::new(move |()| {
        let codec_registry = codec_registry.clone();
        let remote_subnet_config = remote_subnet_config.clone();
        local::boxed_from_fn((
            subnet::run,
            (codec_registry, net_address, remote_subnet_config),
        ))
    });

    let sup_spec = MixedSup::new(OneForOne::new(RestartIntensity {
        max_restarts: 1,
        within:       Duration::from_secs(1),
    }))
    .with_child(
        net_address,
        ChildSpec {
            launcher,
            child_type: ChildType::Permanent,
            init_type: InitType::WithAck {
                start_timeout: CHILD_START_TIMEOUT,
            },
            stop_timeout: CHILD_STOP_TIMEOUT,
        },
    );

    mixed_sup(ctx, sup_spec).await
}
