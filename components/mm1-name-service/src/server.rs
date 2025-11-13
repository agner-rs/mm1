use std::collections::HashMap;
use std::collections::hash_map::Entry::*;
use std::sync::Arc;

use either::Either;
use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_ask::Reply;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log;
use mm1_common::types::{AnyError, Never};
use mm1_core::context::{Bind, BindArgs, InitDone, Messaging};
use mm1_core::envelope::dispatch;
use mm1_proto_ask::Request;
use mm1_proto_named::{
    RegisterErrorKind, RegisterRequest, ResolveRequest, ResolveResponse, UnregisterErrorKind,
    UnregisterRequest,
};
use tokio::time::Instant;

const DEFAULT_INBOX_SIZE: usize = 1024;

pub async fn name_server_actor<Ctx>(
    ctx: &mut Ctx,
    bind_to_networks: impl IntoIterator<Item = NetAddress>,
) -> Result<Never, AnyError>
where
    Ctx: Bind<NetAddress> + Messaging + Reply + InitDone,
{
    for bind_to in bind_to_networks {
        ctx.bind(BindArgs {
            bind_to,
            inbox_size: DEFAULT_INBOX_SIZE,
        })
        .await
        .inspect_err(|reason| log::error!("failed to bind to {}: {}", bind_to, reason))?;

        log::info!("bound to {}", bind_to);
    }
    let () = ctx.init_done(ctx.address()).await;

    let mut state: State = Default::default();

    loop {
        let envelope = ctx.recv().await?;
        dispatch!(match envelope {
            Request::<RegisterRequest::<Arc<str>>> { header, payload } => {
                let RegisterRequest {
                    key,
                    addr,
                    key_props,
                    reg_props,
                } = payload;
                let valid_thru = Instant::now()
                    .checked_add(reg_props.ttl)
                    .unwrap_or_else(|| {
                        log::warn!(
                            "ttl too large, overflow producing valid_thru [ttl: {:?}]",
                            reg_props.ttl
                        );
                        Instant::now()
                    });
                let response = state
                    .register(key, addr, key_props.exclusive, valid_thru)
                    .map_err(|k| ErrorOf::new(k, "registration error"));
                ctx.reply(header, response).await.ok();
            },
            Request::<UnregisterRequest::<Arc<str>>> { header, payload } => {
                let UnregisterRequest { key, addr } = payload;
                let response = state
                    .unregister(key, addr)
                    .map_err(|k| ErrorOf::new(k, "unregistration error"));
                ctx.reply(header, response).await.ok();
            },
            Request::<ResolveRequest::<Arc<str>>> { header, payload } => {
                let ResolveRequest { key } = payload;
                let now = Instant::now();
                let items = state.resolve(&key);
                let (to_evict, to_return) = items
                    .map(|(a, i)| (a, i.saturating_duration_since(now)))
                    .partition::<Vec<_>, _>(|&(_, d)| d.is_zero());

                for (address_to_evict, _) in to_evict {
                    let _ = state.unregister(key.clone(), address_to_evict);
                }
                let response: ResolveResponse = Ok(to_return);

                ctx.reply(header, response).await.ok();
            },
        });
    }
}

#[derive(Default)]
struct State {
    names: HashMap<Arc<str>, Name>,
}

enum Name {
    Exclusive {
        address:      Address,
        registration: Registration,
    },
    Shared(HashMap<Address, Registration>),
}

struct Registration {
    valid_thru: Instant,
}

impl State {
    fn resolve(&self, key: &str) -> impl Iterator<Item = (Address, Instant)> {
        use Either::*;

        let Some(name) = self.names.get(key) else {
            return Left(None.into_iter())
        };

        match name {
            Name::Exclusive {
                address,
                registration,
            } => Left(Some((*address, registration.valid_thru)).into_iter()),
            Name::Shared(entries) => {
                Right(
                    entries
                        .iter()
                        .map(|(address, registration)| (*address, registration.valid_thru)),
                )
            },
        }
    }

    fn register(
        &mut self,
        key: Arc<str>,
        address: Address,
        exclusive: bool,
        valid_thru: Instant,
    ) -> Result<(), RegisterErrorKind> {
        let registration = Registration { valid_thru };
        match (exclusive, self.names.entry(key.clone())) {
            (true, Vacant(v)) => {
                log::info!(
                    "registering {:?} -> {} [valid-thru: {:?}; exclusive; new-entry]",
                    key,
                    address,
                    valid_thru
                );
                v.insert(Name::Exclusive {
                    address,
                    registration,
                });
                Ok(())
            },
            (false, Vacant(v)) => {
                log::info!(
                    "registering {:?} -> {} [valid-thru: {:?}; shared; new-entry]",
                    key,
                    address,
                    valid_thru
                );
                v.insert(Name::Shared(
                    [(address, registration)].into_iter().collect(),
                ));
                Ok(())
            },
            (true, Occupied(mut o)) => {
                match o.get_mut() {
                    Name::Exclusive {
                        address: existing_address,
                        registration: existing_registration,
                    } if *existing_address == address => {
                        log::info!(
                            "registering {:?} -> {} [valid-thru: {:?} -> {:?}; exclusive; \
                             new-entry]",
                            key,
                            address,
                            existing_registration.valid_thru,
                            valid_thru
                        );
                        *existing_registration = registration;
                        Ok(())
                    },
                    _ => {
                        log::warn!("CONFLICT registering {:?} as {} [exclusive]", key, address);
                        Err(RegisterErrorKind::Conflict)
                    },
                }
            },
            (false, Occupied(mut o)) => {
                match o.get_mut() {
                    Name::Exclusive { .. } => {
                        log::warn!("CONFLICT registering {:?} as {} [shared]", key, address);
                        Err(RegisterErrorKind::Conflict)
                    },
                    Name::Shared(registrations) => {
                        let old_registration = registrations.insert(address, registration);
                        log::info!(
                            "registering {:?} -> {} [valid-thru: {:?} -> {:?}; shared; new-entry]",
                            key,
                            address,
                            old_registration.map(|r| r.valid_thru),
                            valid_thru
                        );
                        Ok(())
                    },
                }
            },
        }
    }

    fn unregister(&mut self, key: Arc<str>, address: Address) -> Result<(), UnregisterErrorKind> {
        match self.names.entry(key) {
            Vacant(_) => Err(UnregisterErrorKind::NotFound),
            Occupied(mut o) => {
                match o.get_mut() {
                    Name::Exclusive {
                        address: existing_address,
                        ..
                    } if *existing_address == address => {
                        o.remove();
                        Ok(())
                    },
                    Name::Exclusive { .. } => Err(UnregisterErrorKind::NotFound),
                    Name::Shared(registrations) => {
                        registrations.remove(&address);
                        if registrations.is_empty() {
                            o.remove();
                        }
                        Ok(())
                    },
                }
            },
        }
    }
}
