use mm1::runnable::local;

const MESSAGE_SIZE: usize = 64;

fn main() {
    let config = Config::<MESSAGE_SIZE> {
        client_actors_count: std::env::var("CLIENT_ACTORS_COUNT")
            .unwrap_or("1".into())
            .parse()
            .unwrap(),
        server_forks_count:  std::env::var("SERVER_FORKS_COUNT")
            .unwrap_or("1".into())
            .parse()
            .unwrap(),
        requests_count:      std::env::var("REQUESTS_COUNT")
            .unwrap_or("1".into())
            .parse()
            .unwrap(),
    };

    let mm1_config = serde_yaml::from_str(
        r#"
            local_subnets:
                - net: <:>/0
                  kind: auto
            actor:
                inbox_size: 1024
                /:
                    _:
                        netmask: 32
                
        "#,
    )
    .expect("parse-config error");
    let rt = mm1::runtime::Rt::create(mm1_config).expect("Rt::create");

    let main = local::boxed_from_fn((actors::main, (config,)));
    rt.run(main).expect("main actor failure");
}

#[derive(Debug, Clone)]
struct Config<const MESSAGE_SIZE: usize> {
    server_forks_count:  usize,
    client_actors_count: usize,
    requests_count:      usize,
}

mod actors {
    use std::hash::{Hash, Hasher};
    use std::time::Duration;

    use eyre::Context;
    use futures::future;
    use hdrhistogram::Histogram;
    use mm1::address::Address;
    use mm1::ask::proto::RequestHeader;
    use mm1::ask::{Ask, Reply};
    use mm1::common::error::{AnyError, ExactTypeDisplayChainExt};
    use mm1::core::context::{Fork, InitDone, Messaging, Quit, Start};
    use mm1::proto::message;
    use mm1::runnable::local;
    use mm1::server;
    use mm1::server::behaviour::{OnRequest, Outcome};
    use tokio::time::Instant;

    use crate::Config;

    #[message]
    struct Request<P> {
        #[serde(with = "mm1::common::serde::no_serde")]
        payload: P,
    }
    #[message]
    struct Response<P> {
        #[serde(with = "mm1::common::serde::no_serde")]
        payload: P,
    }

    pub(super) async fn main<Ctx, const MESSAGE_SIZE: usize>(
        ctx: &mut Ctx,
        config: Config<MESSAGE_SIZE>,
    ) -> Result<(), AnyError>
    where
        Ctx: Ask
            + Fork
            + InitDone
            + Messaging
            + Reply
            + Start<local::BoxedRunnable<Ctx>>
            + Quit
            + Send
            + Sync,
    {
        async fn inner<Ctx, const MESSAGE_SIZE: usize>(
            ctx: &mut Ctx,
            config: Config<MESSAGE_SIZE>,
        ) -> Result<(), AnyError>
        where
            Ctx: Ask
                + Fork
                + InitDone
                + Messaging
                + Reply
                + Start<local::BoxedRunnable<Ctx>>
                + Quit
                + Send
                + Sync,
        {
            eprintln!("hello");

            let mut fork_addresses = vec![];
            for _ in 0..config.server_forks_count {
                let fork_ctx = ctx.fork().await.wrap_err("ctx.fork")?;
                fork_addresses.push(fork_ctx.address());
                fork_ctx.run(server_fork::<_, MESSAGE_SIZE>).await;
            }

            eprintln!("server forks created");

            let mut client_addresses = vec![];
            for _ in 0..config.client_actors_count {
                let client = local::boxed_from_fn(client::<Ctx, MESSAGE_SIZE>);
                let client_address = ctx
                    .start(client, true, Duration::from_secs(1))
                    .await
                    .wrap_err("ctx.start")?;
                client_addresses.push(client_address);
            }

            eprintln!("client actors started");

            let mut stats = vec![];

            for client in client_addresses {
                let mut f = ctx.fork().await.wrap_err("ctx.fork")?;
                let config = config.clone();
                let fork_addresses = fork_addresses.clone();
                stats.push(async move {
                    f.ask::<_, Response<Vec<Duration>>>(
                        client,
                        Request {
                            payload: (config, fork_addresses),
                        },
                        Duration::from_mins(10),
                    )
                    .await
                    .wrap_err("ctx.ask")
                });
            }

            eprintln!("running...");

            let mut hist =
                Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).wrap_err("Histogram::new")?;
            future::try_join_all(stats)
                .await
                .wrap_err("try_join")?
                .into_iter()
                .flat_map(|Response { payload }| payload)
                .try_for_each(|dt| hist.record(dt.as_micros() as u64).wrap_err("hist.record"))
                .wrap_err("try_for_each")?;
            eprintln!("count: {}", hist.len());
            eprintln!("p50  :  {} µs", hist.value_at_quantile(0.50));
            eprintln!("p90  :  {} µs", hist.value_at_quantile(0.90));
            eprintln!("p95  :  {} µs", hist.value_at_quantile(0.95));
            eprintln!("p99  :  {} µs", hist.value_at_quantile(0.99));
            eprintln!("p99.9:  {} µs", hist.value_at_quantile(0.999));
            eprintln!("max  :  {} µs", hist.max());

            eprintln!("done!");

            Ok(())
        }

        inner(ctx, config)
            .await
            .inspect_err(|e| eprintln!("{}", e.as_display_chain()))
    }

    async fn client<Ctx, const MESSAGE_SIZE: usize>(ctx: &mut Ctx) -> Result<(), AnyError>
    where
        Ctx: Ask
            + Fork
            + InitDone
            + Messaging
            + Reply
            + Start<local::BoxedRunnable<Ctx>>
            + Quit
            + Send
            + Sync,
    {
        ctx.init_done(ctx.address()).await;

        server::new::<Ctx>()
            .behaviour(Client::<MESSAGE_SIZE>)
            .req::<Request<(Config<MESSAGE_SIZE>, Vec<Address>)>>()
            .run(ctx)
            .await?;

        Ok(())
    }

    async fn server_fork<Ctx, const MESSAGE_SIZE: usize>(mut ctx: Ctx) -> Result<(), AnyError>
    where
        Ctx: Fork + InitDone + Messaging + Start<local::BoxedRunnable<Ctx>> + Quit + Send + Sync,
    {
        ctx.init_done(ctx.address()).await;

        server::new::<Ctx>()
            .behaviour(Server::<MESSAGE_SIZE>)
            .req::<Request<[u8; MESSAGE_SIZE]>>()
            .run(&mut ctx)
            .await?;

        Ok(())
    }

    struct Client<const MESSAGE_SIZE: usize>;
    struct Server<const MESSAGE_SIZE: usize>;

    impl<Ctx, const MESSAGE_SIZE: usize>
        OnRequest<Ctx, Request<(Config<MESSAGE_SIZE>, Vec<Address>)>> for Client<MESSAGE_SIZE>
    where
        Ctx: Ask
            + Fork
            + InitDone
            + Messaging
            + Reply
            + Start<local::BoxedRunnable<Ctx>>
            + Quit
            + Send
            + Sync,
    {
        type Rs = Response<Vec<Duration>>;

        async fn on_request(
            &mut self,
            ctx: &mut Ctx,
            _reply_to: RequestHeader,
            request: Request<(Config<MESSAGE_SIZE>, Vec<Address>)>,
        ) -> Result<Outcome<Request<(Config<MESSAGE_SIZE>, Vec<Address>)>, Self::Rs>, AnyError>
        {
            let Request {
                payload: (config, mut servers),
            } = request;
            assert!(!servers.is_empty());

            let mut response_times = vec![];

            servers.sort_by_key(|s| {
                let mut h = std::hash::DefaultHasher::new();
                (s, ctx.address()).hash(&mut h);
                h.finish()
            });

            for (_, server) in (0..config.requests_count).zip(servers.into_iter().cycle()) {
                let t0 = Instant::now();
                ctx.ask::<_, Response<[u8; MESSAGE_SIZE]>>(
                    server,
                    Request {
                        payload: [0u8; MESSAGE_SIZE],
                    },
                    Duration::from_secs(1),
                )
                .await
                .wrap_err("ctx.ask")?;
                response_times.push(t0.elapsed());
            }

            Ok(Outcome::reply(Response {
                payload: response_times,
            }))
        }
    }

    impl<Ctx, const MESSAGE_SIZE: usize> OnRequest<Ctx, Request<[u8; MESSAGE_SIZE]>>
        for Server<MESSAGE_SIZE>
    where
        Ctx: Ask
            + Fork
            + InitDone
            + Messaging
            + Reply
            + Start<local::BoxedRunnable<Ctx>>
            + Quit
            + Send
            + Sync,
    {
        type Rs = Response<[u8; MESSAGE_SIZE]>;

        async fn on_request(
            &mut self,
            _ctx: &mut Ctx,
            _reply_to: RequestHeader,
            request: Request<[u8; MESSAGE_SIZE]>,
        ) -> Result<Outcome<Request<[u8; MESSAGE_SIZE]>, Self::Rs>, AnyError> {
            let Request { payload } = request;
            Ok(Outcome::reply(Response { payload }))
        }
    }
}
