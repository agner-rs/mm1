use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eyre::Context;
use mm1::address::{Address, NetAddress, NetMask};
use mm1::common::error::AnyError;
use mm1::common::log::{error, info};
use mm1::core::context::BindArgs;
use mm1::core::envelope::dispatch;
use mm1::multinode::Protocol;
use mm1::runnable::local;
use mm1::runtime::Rt;
use mm1::runtime::config::Mm1NodeConfig;
use mm1::timer::v1::OneshotTimer;
use mm1_multinode::actors::context::ActorContext;
use mm1_proto_network_management as nm;
use mm1_proto_well_known::MULTINODE_MANAGER;
use serde_json::json;
use structopt::StructOpt;
use tokio::sync::mpsc;
use url::Url;

#[derive(StructOpt)]
struct Args {
    #[structopt(long)]
    local: NetAddress,

    #[structopt(long)]
    cookie: Option<String>,

    #[structopt(long, short)]
    proto: Vec<String>,

    #[structopt(long)]
    bind: Vec<Url>,

    #[structopt(long)]
    connect: Vec<Url>,

    #[structopt(long = "dst", short = "d")]
    destinations: Vec<Address>,

    #[structopt(long = "receive-at", short = "r")]
    receive_at: Address,

    #[structopt(long = "tick", short = "t")]
    tick_interval: humantime::Duration,

    #[structopt(long = "msg", short = "m", default_value = "64")]
    msg_size: usize,
}

fn main() {
    if let Err(failure) = run(StructOpt::from_args()) {
        eprintln!("failure:");
        for cause in failure.chain() {
            eprintln!(" - {}", cause);
        }
        std::process::exit(1)
    }
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_multinode::*=TRACE".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}

fn run(args: Args) -> Result<(), AnyError> {
    let _ = dotenv::dotenv();
    let _ = mm1_logger::init(&logger_config());

    let Args {
        local,
        bind,
        proto,
        cookie,
        connect,
        destinations,
        receive_at,
        msg_size,
        tick_interval,
    } = args;

    let authc_config = if let Some(cookie) = cookie {
        json!({
            "cookie": cookie,
        })
    } else {
        json!("trusted")
    };

    let inbound = bind
        .into_iter()
        .map(|bind_addr| {
            json!({
                "proto": proto,
                "addr": bind_addr,
                "authc": authc_config,
            })
        })
        .collect::<Vec<_>>();
    let outbound = connect
        .into_iter()
        .map(|dst_addr| {
            json!({
                "proto": proto,
                "addr": dst_addr,
                "authc": authc_config,
            })
        })
        .collect::<Vec<_>>();
    let config = json!({
        "local_subnets": [
            {
                "net": local,
                "kind": "auto"
            }
        ],
        "inbound": inbound,
        "outbound": outbound,
    });

    let config: Mm1NodeConfig = serde_json::from_value(config).wrap_err("config from json")?;

    let (tx_actor_failure, mut rx_actor_failure) = mpsc::unbounded_channel();

    let rt = Rt::create(config)
        .wrap_err("Rt::create")?
        .with_actor_failure_sink(tx_actor_failure);

    let log_actor_failures = async move {
        while let Some((address, actor_failure)) = rx_actor_failure.recv().await {
            let actor_failure_chain = actor_failure
                .chain()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(" <- ");
            error!(
                "actor failure [address: {}]: {}",
                address, actor_failure_chain
            );
        }
    };
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(log_actor_failures)
    });

    rt.run(local::boxed_from_fn((
        main_actor,
        (*tick_interval, msg_size, receive_at, destinations),
    )))
    .wrap_err("rt.run")?;

    Ok(())
}

async fn main_actor<Ctx>(
    ctx: &mut Ctx,
    tick_interval: Duration,
    msg_size: usize,
    receive_at: Address,
    destinations: Vec<Address>,
) -> Result<(), AnyError>
where
    Ctx: ActorContext,
{
    ctx.bind(BindArgs {
        bind_to:    NetAddress {
            address: receive_at,
            mask:    NetMask::MAX,
        },
        inbox_size: 1024,
    })
    .await
    .wrap_err("ctx.bind")?;

    info!("HELLO! I'm {} [also: {}]", ctx.address(), receive_at);

    let protocol = Protocol::new()
        .with_type::<proto::M1>()
        .with_type::<proto::M2>();
    let register_protocol_request = nm::protocols::RegisterProtocolRequest {
        name: "proto".into(),
        protocol,
    };
    let () = ctx
        .ask::<_, nm::protocols::RegisterProtocolResponse>(
            MULTINODE_MANAGER,
            register_protocol_request,
            Duration::from_millis(100),
        )
        .await
        .wrap_err("ctx.ask::<nm::RegisterProtocol>")?
        .wrap_err("nm::RegisterProtocol")?;

    let mut timer_api = OneshotTimer::create(ctx)
        .await
        .wrap_err("OnesthoTimer::create")?;

    ctx.tell(ctx.address(), proto::Tick)
        .await
        .wrap_err("ctx.tell to self")?;

    loop {
        let envelope = ctx.recv().await.wrap_err("ctx.recv")?;

        dispatch!(match envelope {
            proto::Tick => {
                for d in destinations.iter().copied() {
                    ctx.tell(d, proto::M1(ctx.address(), now(), vec![0u8; msg_size]))
                        .await
                        .ok();
                }
                timer_api
                    .schedule_once_after(tick_interval, proto::Tick)
                    .await
                    .wrap_err("timer_api.schedule_once_after")?;
            },
            proto::M1(reply_to, sent_at, data) => {
                info!(
                    "M1 from {} [dt: {:?}]",
                    reply_to,
                    Duration::from_micros(now().saturating_sub(sent_at))
                );
                ctx.tell(reply_to, proto::M2(ctx.address(), now(), data))
                    .await
                    .ok();
            },
            proto::M2(replied_by, sent_at, data) => {
                info!(
                    "M2 from {} [dt: {:?}; len: {}]",
                    replied_by,
                    Duration::from_micros(now().saturating_sub(sent_at)),
                    data.len()
                );
            },
            a_message @ _ => {
                info!("RECEIVED {:?}", a_message);
            },
        });
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("okay")
        .as_micros() as u64
}

mod proto {
    use mm1::address::Address;
    use mm1::proto::message;

    #[message]
    pub struct Tick;

    #[message]
    pub struct M1(pub Address, pub u64, pub Vec<u8>);
    #[message]
    pub struct M2(pub Address, pub u64, pub Vec<u8>);
}
