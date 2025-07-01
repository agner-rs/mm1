use std::str::FromStr;
use std::time::Duration;

use mm1::address::{Address, NetAddress, NetMask};
use mm1::common::error::AnyError;
use mm1::common::log::info;
use mm1::core::context::{Bind, BindArgs, Fork, Messaging, Tell};
use mm1::core::envelope::dispatch;
use mm1::message_codec::Codec;
use mm1::proto::message;
use mm1::runnable::local;
use mm1::runtime::Rt;
use mm1_logger::Level;

fn main() -> Result<(), AnyError> {
    let _ = mm1_logger::init(&mm1_logger::LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: ["*=info"]
            .into_iter()
            .map(FromStr::from_str)
            .collect::<Result<_, _>>()?,
    });

    let config_path = std::env::var("CONFIG")?;
    let config_text = std::fs::read_to_string(&config_path)?;
    let config = serde_yaml::from_str(&config_text)?;

    let bind: Address = std::env::var("BIND")?.parse()?;
    let peer: Address = std::env::var("PEER")?.parse()?;

    Rt::create(config)?
        .with_codec("full", Codec::new().with_type::<AMessage>())
        .run(local::boxed_from_fn((main_actor, (bind, peer))))?;

    Ok(())
}

async fn main_actor<C>(ctx: &mut C, bind: Address, peer: Address) -> Result<(), AnyError>
where
    C: Bind<NetAddress> + Fork + Messaging,
{
    ctx.bind(BindArgs {
        bind_to:    NetAddress {
            address: bind,
            mask:    NetMask::M_64,
        },
        inbox_size: 1024,
    })
    .await?;

    ctx.fork()
        .await?
        .run(move |mut ctx| {
            async move {
                for idx in 1.. {
                    let outcome = ctx
                        .tell(
                            peer,
                            AMessage {
                                reply_to: bind,
                                idx,
                            },
                        )
                        .await;
                    info!("sending [{idx}] to {peer}: {outcome:?}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        })
        .await;

    loop {
        let envelope = ctx.recv().await?;
        dispatch!(match envelope {
            a_message @ AMessage { .. } => {
                info!("received AMessage: {a_message:?}");
            },
        })
    }
}

#[derive(Debug)]
#[message]
struct AMessage {
    reply_to: Address,
    idx:      usize,
}
