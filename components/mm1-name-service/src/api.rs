#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_ask::Ask;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log;
use mm1_common::types::Never;
use mm1_core::context::Fork;
use mm1_proto_named::{
    KeyProps, RegProps, RegisterErrorKind, RegisterRequest, RegisterResponse, ResolveErrorKind,
    ResolveRequest, ResolveResponse,
};
use tokio::time::{self, Instant};

pub const DEFAULT_RESOLVE_TIMEOUT: Duration = Duration::from_millis(10);
pub const DEFAULT_REGISTER_TIMEOUT: Duration = Duration::from_millis(10);
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
pub const DEFAULT_TTL: Duration = Duration::from_secs(2);

pub struct Registration<Ctx> {
    ctx:                  Ctx,
    name_server:          Address,
    key:                  Arc<str>,
    addr:                 Address,
    pub register_timeout: Duration,
    pub refresh_interval: Duration,
    pub ttl:              Duration,
    pub exclusive:        bool,
}

pub struct Resolver<Ctx> {
    ctx:                 Ctx,
    name_server:         Address,
    pub resolve_timeout: Duration,
}

pub struct Resolution {
    key:          Arc<str>,
    requested_at: Instant,
    entries:      Vec<(Address, Duration)>,
}

impl<Ctx> Registration<Ctx> {
    pub fn new(ctx: Ctx, name_server: Address, name: impl Into<Arc<str>>, addr: Address) -> Self {
        let key = name.into();
        Self {
            ctx,
            name_server,
            key,
            addr,
            register_timeout: DEFAULT_REGISTER_TIMEOUT,
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            ttl: DEFAULT_TTL,
            exclusive: true,
        }
    }

    pub fn with_register_timeout(self, register_timeout: Duration) -> Self {
        Self {
            register_timeout,
            ..self
        }
    }

    pub fn with_refresh_interval(self, refresh_interval: Duration) -> Self {
        Self {
            refresh_interval,
            ..self
        }
    }

    pub fn with_ttl(self, ttl: Duration) -> Self {
        Self { ttl, ..self }
    }

    pub fn with_exclusive(self, exclusive: bool) -> Self {
        Self { exclusive, ..self }
    }
}
impl<Ctx> Registration<Ctx>
where
    Ctx: Ask,
{
    pub async fn run(self) -> Result<Never, ErrorOf<RegisterErrorKind>> {
        let key = self.key.clone();
        let addr = self.addr;
        self.run_inner()
            .await
            .inspect_err(|reason| log::warn!("failed to register {} as {}: {}", addr, key, reason))
    }

    async fn run_inner(self) -> Result<Never, ErrorOf<RegisterErrorKind>> {
        let Self {
            mut ctx,
            name_server,
            key,
            addr,
            register_timeout,
            refresh_interval,
            ttl,
            exclusive,
        } = self;
        loop {
            let name = key.as_ref();
            let key = key.clone();
            let key_props = KeyProps { exclusive };
            let reg_props = RegProps { ttl };
            let request = RegisterRequest {
                key,
                addr,
                key_props,
                reg_props,
            };
            let response: RegisterResponse = ctx
                .ask_nofork(name_server, request, register_timeout)
                .await
                .map_err(|e| e.map_kind(|_| RegisterErrorKind::Internal))?;
            let () = response?;

            log::debug!(
                "registered {} as {}. Refresh in {:?}",
                addr,
                name,
                refresh_interval
            );

            time::sleep(refresh_interval).await;
        }
    }
}

impl<Ctx> Resolver<Ctx> {
    pub fn new(ctx: Ctx, name_server: Address) -> Self {
        Self {
            ctx,
            name_server,
            resolve_timeout: DEFAULT_RESOLVE_TIMEOUT,
        }
    }

    pub fn with_timeout(self, resolve_timeout: Duration) -> Self {
        Self {
            resolve_timeout,
            ..self
        }
    }

    pub async fn resolve(
        &mut self,
        name: impl Into<Arc<str>>,
    ) -> Result<Resolution, ErrorOf<ResolveErrorKind>>
    where
        Ctx: Ask + Fork,
    {
        let key = name.into();
        let request = ResolveRequest { key: key.clone() };
        let requested_at = Instant::now();
        let response: ResolveResponse = self
            .ctx
            .ask(self.name_server, request, self.resolve_timeout)
            .await
            .map_err(|e| e.map_kind(|_| ResolveErrorKind::Internal))?;
        let mut entries = response?;

        entries.sort_unstable_by_key(|e| {
            use std::hash::{DefaultHasher, Hash, Hasher};

            let mut h = DefaultHasher::new();
            (e, requested_at).hash(&mut h);
            h.finish()
        });

        let resolution = Resolution {
            key,
            requested_at,
            entries,
        };

        Ok(resolution)
    }
}

impl Resolution {
    pub async fn refresh<Ctx>(
        &mut self,
        name_resolver: &mut Resolver<Ctx>,
    ) -> Result<(), ErrorOf<ResolveErrorKind>>
    where
        Ctx: Ask + Fork,
    {
        *self = name_resolver.resolve(self.key.clone()).await?;
        Ok(())
    }

    pub async fn wait<Ctx>(
        &mut self,
        name_resolver: &mut Resolver<Ctx>,
        retries: usize,
        interval: Duration,
    ) -> Result<(), ErrorOf<ResolveErrorKind>>
    where
        Ctx: Ask + Fork,
    {
        for _ in 0..retries {
            let updated = name_resolver.resolve(self.key.clone()).await?;
            if !updated.entries.is_empty() {
                *self = updated;

                log::debug!(
                    "resolved {} into {} addresses",
                    self.key,
                    self.entries.len()
                );

                return Ok(())
            }

            log::debug!(
                "resolved {} into no addresses. Retrying in {:?}",
                self.key,
                interval
            );
            time::sleep(interval).await;
        }

        log::warn!(
            "resolution for {} failed [retries: {}; interval: {:?}]",
            self.key,
            retries,
            interval
        );
        Err(ErrorOf::new(ResolveErrorKind::Internal, "out of retries"))
    }

    pub fn first(&self) -> Option<Address> {
        let now = Instant::now();

        self.entries
            .first()
            .filter(|(_, d)| self.still_valid(now, *d))
            .map(|(a, _)| *a)
    }

    pub fn any(&self) -> Option<Address> {
        let now = Instant::now();

        self.entries
            .iter()
            .filter(|(_, d)| self.still_valid(now, *d))
            .map(|(a, _)| *a)
            .next()
    }

    fn still_valid(&self, now: Instant, d: Duration) -> bool {
        self.requested_at
            .checked_add(d)
            .is_some_and(|deadline| now < deadline)
    }
}
