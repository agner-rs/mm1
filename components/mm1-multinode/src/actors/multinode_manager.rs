use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bimap::BiMap;
use eyre::Context;
use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::subnet::NetAddress;
use mm1_common::errors::error_of::ErrorOf;
use mm1_common::log::{debug, error, info, trace};
use mm1_common::types::{AnyError, Never};
use mm1_core::context::BindArgs;
use mm1_proto::message;
use mm1_proto_ask::RequestHeader;
use mm1_proto_network_management::{iface as i, protocols as p};
use mm1_proto_sup::uniform as uni_sup;
use mm1_proto_system::WatchRef;
use mm1_proto_well_known::MULTINODE_MANAGER;
use mm1_runnable::local;
use mm1_server::{OnMessage, OnRequest, Outcome};
use mm1_sup::common::child_spec::{ChildSpec, InitType};
use mm1_sup::common::factory::ActorFactoryMut;
use mm1_sup::uniform::{UniformSup, child_type};
use mm1_timer::v1::{OneshotKey, OneshotTimer};
use slotmap::SlotMap;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UnixStream};
use {mm1_proto_network_management as nm, mm1_proto_system as sys};

use crate::actors::context::ActorContext;
use crate::codec::{self, Protocol};
use crate::proto::{SetRoute, SubscribeToRoutesRequest, SubscribeToRoutesResponse};
use crate::protocol_registry::ProtocolRegistry;
use crate::route_registry::RouteRegistry;

pub const INBOX_SIZE: usize = 1024;
const ACCEPTOR_START_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECTOR_START_TIMEOUT: Duration = Duration::from_secs(1);

pub async fn run<Ctx>(ctx: &mut Ctx) -> Result<Never, AnyError>
where
    Ctx: ActorContext,
{
    let timer_api = OneshotTimer::create(ctx)
        .await
        .wrap_err("OneshotTimer::create")?;

    let uds_connection_sup = start_connection_sup::<_, UnixStream>(ctx, ctx.address())
        .await
        .wrap_err("start_connection_sup::<UnixStream>")?;
    debug!("uds-connection-sup: {}", uds_connection_sup);

    let uds_connector_sup = start_uds_connector_sup(ctx, uds_connection_sup)
        .await
        .wrap_err("start_uds_connector_sup")?;
    debug!("uds-connector-sup: {}", uds_connector_sup);

    let uds_acceptor_sup = start_uds_acceptor_sup(ctx, uds_connection_sup)
        .await
        .wrap_err("start_uds_acceptor_sup")?;
    debug!("uds-acceptor-sup: {}", uds_acceptor_sup);

    let tcp_connection_sup = start_connection_sup::<_, TcpStream>(ctx, ctx.address())
        .await
        .wrap_err("start_connection_sup::<TcpStream>")?;
    debug!("tcp-connection-sup: {}", tcp_connection_sup);

    let tcp_connector_sup = start_tcp_connector_sup(ctx, tcp_connection_sup)
        .await
        .wrap_err("start_tcp_connector_sup")?;
    debug!("tcp-connector-sup: {}", tcp_connector_sup);

    let tcp_acceptor_sup = start_tcp_acceptor_sup(ctx, tcp_connection_sup)
        .await
        .wrap_err("start_tcp_acceptor_sup")?;
    debug!("tcp-acceptor-sup: {}", tcp_acceptor_sup);

    let subnet_ingress_sup = start_subnet_ingress_sup(ctx)
        .await
        .wrap_err("start_subnet_ingress_sup")?;
    debug!("subnet-ingress-sup: {}", subnet_ingress_sup);

    ctx.bind(BindArgs {
        bind_to:    MULTINODE_MANAGER.into(),
        inbox_size: INBOX_SIZE,
    })
    .await
    .wrap_err("bind MULTINODE_MANAGER")?;
    ctx.init_done(ctx.address()).await;
    info!("MULTINODE_MANAGER started");

    mm1_server::new::<Ctx>()
        .behaviour(MultinodeManager {
            timer_api,
            local_subnets: Default::default(),
            route_subscribers: Default::default(),
            route_gws: Default::default(),
            route_registry: Default::default(),
            protocol_registry: Default::default(),
            protocol_waitlist: Default::default(),
            tcp_connector_sup,
            tcp_acceptor_sup,
            uds_connector_sup,
            uds_acceptor_sup,
            subnet_ingress_sup,
            subnet_ingress_workers: Default::default(),
            tcp_acceptors: Default::default(),
            tcp_connectors: Default::default(),
            uds_acceptors: Default::default(),
            uds_connectors: Default::default(),
        })
        .req::<p::RegisterLocalSubnetRequest>()
        .req::<i::ConnectRequest<SocketAddr>>()
        .req::<i::ConnectRequest<Box<Path>>>()
        .req::<i::BindRequest<SocketAddr>>()
        .req::<i::BindRequest<Box<Path>>>()
        .req::<p::RegisterProtocolRequest<Protocol>>()
        .req::<p::UnregisterProtocolRequest>()
        .req::<p::RegisterOpaqueMessageRequest>()
        .req::<p::GetMessageNameRequest>()
        .req::<p::GetProtocolByNameRequest>()
        .req::<p::GetLocalSubnetsRequest>()
        .req::<p::ResolveTypeIdRequest>()
        .msg::<WaitlistTimeoutElapsed>()
        .req::<SubscribeToRoutesRequest>()
        .msg::<SetRoute>()
        .msg::<sys::Down>()
        .run(ctx)
        .await
        .wrap_err("server::run")?;

    Err(eyre::format_err!("multinode-manager exiting"))
}

struct Ifaces<K, A>
where
    K: slotmap::Key,
{
    entries:       SlotMap<K, IfaceEntry<A>>,
    by_iface_addr: HashMap<A, K>,
}

impl<K, A> Default for Ifaces<K, A>
where
    K: slotmap::Key,
{
    fn default() -> Self {
        Self {
            entries:       Default::default(),
            by_iface_addr: Default::default(),
        }
    }
}

#[allow(dead_code)]
struct IfaceEntry<A> {
    iface_address: A,
    actor_address: Address,
}

slotmap::new_key_type! {
    struct TcpAcceptorKey;
    struct TcpConnectorKey;

    struct UdsAcceptorKey;
    struct UdsConnectorKey;
}

slotmap::new_key_type! {struct WaitlistKey;}

#[derive(Default)]
struct Waitlist {
    entries:     SlotMap<WaitlistKey, WaitlistEntry>,
    by_protocol: BTreeSet<(nm::ProtocolName, Option<WaitlistKey>)>,
}
#[test]
fn none_is_less_than_some() {
    assert!(Option::<usize>::None < Option::<usize>::Some(1));
}

struct WaitlistEntry {
    protocol:  nm::ProtocolName,
    reply_to:  RequestHeader,
    timer_key: Option<OneshotKey>,
}

#[message(base_path = ::mm1_proto)]
struct WaitlistTimeoutElapsed {
    waitlist_key: WaitlistKey,
}

struct MultinodeManager<Ctx> {
    timer_api:              OneshotTimer<Ctx>,
    local_subnets:          BTreeSet<AddressRange>,
    route_registry:         RouteRegistry,
    route_subscribers:      HashMap<sys::WatchRef, Address>,
    route_gws:              BiMap<WatchRef, Address>,
    protocol_registry:      ProtocolRegistry,
    protocol_waitlist:      Waitlist,
    tcp_connector_sup:      Address,
    tcp_acceptor_sup:       Address,
    uds_connector_sup:      Address,
    uds_acceptor_sup:       Address,
    subnet_ingress_sup:     Address,
    subnet_ingress_workers: BTreeMap<AddressRange, Address>,
    tcp_acceptors:          Ifaces<TcpAcceptorKey, SocketAddr>,
    uds_acceptors:          Ifaces<UdsAcceptorKey, Box<Path>>,
    tcp_connectors:         Ifaces<TcpConnectorKey, SocketAddr>,
    uds_connectors:         Ifaces<UdsConnectorKey, Box<Path>>,
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::RegisterLocalSubnetRequest> for MultinodeManager<Ctx> {
    type Rs = p::RegisterLocalSubnetResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::RegisterLocalSubnetRequest,
    ) -> Result<mm1_server::Outcome<Self::Rs>, AnyError> {
        let p::RegisterLocalSubnetRequest { net } = request;
        let Self { local_subnets, .. } = self;

        local_subnets.insert(net.into());
        info!("registered local subnet: {}", net,);

        Ok(Outcome::Reply(Ok(())))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, i::ConnectRequest<SocketAddr>> for MultinodeManager<Ctx> {
    type Rs = i::ConnectResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: i::ConnectRequest<SocketAddr>,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        use std::collections::hash_map::Entry::*;

        let i::ConnectRequest {
            dst_address: iface_address,
            protocol_names,
            options,
        } = request;

        let Self {
            tcp_connector_sup: connector_sup,
            tcp_connectors: connectors,
            ..
        } = self;
        let Ifaces {
            entries,
            by_iface_addr,
        } = connectors;

        let reply_with: i::ConnectResponse = 'reply: {
            let Vacant(by_dst_addr) = by_iface_addr.entry(iface_address) else {
                break 'reply Err(ErrorOf::new(
                    i::ConnectErrorKind::DuplicateDstAddr,
                    "address already being connected to",
                ))
            };

            let actor_address = ctx
                .ask::<_, uni_sup::StartResponse>(
                    *connector_sup,
                    uni_sup::StartRequest {
                        args: (iface_address, protocol_names, options),
                    },
                    CONNECTOR_START_TIMEOUT,
                )
                .await
                .wrap_err("ask")?
                .wrap_err("uni_sup::Start")?;
            let connector_key = entries.insert(IfaceEntry {
                iface_address,
                actor_address,
            });

            by_dst_addr.insert(connector_key);

            Ok(())
        };

        Ok(Outcome::Reply(reply_with))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, i::ConnectRequest<Box<Path>>> for MultinodeManager<Ctx> {
    type Rs = i::ConnectResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: i::ConnectRequest<Box<Path>>,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        use std::collections::hash_map::Entry::*;

        let i::ConnectRequest {
            dst_address: iface_address,
            protocol_names,
            options,
        } = request;

        let Self {
            uds_connector_sup: connector_sup,
            uds_connectors: connectors,
            ..
        } = self;
        let Ifaces {
            entries,
            by_iface_addr,
        } = connectors;

        let reply_with: i::ConnectResponse = 'reply: {
            let Vacant(by_dst_addr) = by_iface_addr.entry(iface_address.clone()) else {
                break 'reply Err(ErrorOf::new(
                    i::ConnectErrorKind::DuplicateDstAddr,
                    "address already being connected to",
                ))
            };

            let actor_address = ctx
                .ask::<_, uni_sup::StartResponse>(
                    *connector_sup,
                    uni_sup::StartRequest {
                        args: (iface_address.clone(), protocol_names, options),
                    },
                    CONNECTOR_START_TIMEOUT,
                )
                .await
                .wrap_err("ask")?
                .wrap_err("uni_sup::Start")?;
            let connector_key = entries.insert(IfaceEntry {
                iface_address,
                actor_address,
            });

            by_dst_addr.insert(connector_key);

            Ok(())
        };

        Ok(Outcome::Reply(reply_with))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, i::BindRequest<SocketAddr>> for MultinodeManager<Ctx> {
    type Rs = i::BindResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: i::BindRequest<SocketAddr>,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        use std::collections::hash_map::Entry::*;

        let i::BindRequest {
            bind_address: iface_address,
            protocol_names,
            options,
        } = request;

        let Self {
            tcp_acceptor_sup: acceptor_sup,
            tcp_acceptors: acceptors,
            ..
        } = self;
        let Ifaces {
            entries,
            by_iface_addr,
        } = acceptors;

        let reply_with: i::BindResponse = 'reply: {
            let Vacant(by_bind_addr) = by_iface_addr.entry(iface_address) else {
                break 'reply Err(ErrorOf::new(
                    i::BindErrorKind::DuplicateBindAddr,
                    "address already bound",
                ))
            };

            let actor_address = ctx
                .ask::<_, uni_sup::StartResponse>(
                    *acceptor_sup,
                    uni_sup::StartRequest {
                        args: (iface_address, protocol_names, options),
                    },
                    ACCEPTOR_START_TIMEOUT,
                )
                .await
                .wrap_err("ask")?
                .wrap_err("uni_sup::Start")?;

            let acceptor_key = entries.insert(IfaceEntry {
                iface_address,
                actor_address,
            });

            by_bind_addr.insert(acceptor_key);

            Ok(())
        };

        Ok(Outcome::Reply(reply_with))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, i::BindRequest<Box<Path>>> for MultinodeManager<Ctx> {
    type Rs = i::BindResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: i::BindRequest<Box<Path>>,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        use std::collections::hash_map::Entry::*;

        let i::BindRequest {
            bind_address: iface_address,
            protocol_names,
            options,
        } = request;

        let Self {
            uds_acceptor_sup: acceptor_sup,
            uds_acceptors: acceptors,
            ..
        } = self;
        let Ifaces {
            entries,
            by_iface_addr,
        } = acceptors;

        let reply_with: i::BindResponse = 'reply: {
            let Vacant(by_bind_addr) = by_iface_addr.entry(iface_address.clone()) else {
                break 'reply Err(ErrorOf::new(
                    i::BindErrorKind::DuplicateBindAddr,
                    "address already bound",
                ))
            };

            let actor_address = ctx
                .ask::<_, uni_sup::StartResponse>(
                    *acceptor_sup,
                    uni_sup::StartRequest {
                        args: (iface_address.clone(), protocol_names, options),
                    },
                    ACCEPTOR_START_TIMEOUT,
                )
                .await
                .wrap_err("ask")?
                .wrap_err("uni_sup::Start")?;

            let acceptor_key = entries.insert(IfaceEntry {
                iface_address,
                actor_address,
            });

            by_bind_addr.insert(acceptor_key);

            Ok(())
        };

        Ok(Outcome::Reply(reply_with))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::RegisterProtocolRequest<Protocol>>
    for MultinodeManager<Ctx>
{
    type Rs = p::RegisterProtocolResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        reply_to: RequestHeader,
        request: p::RegisterProtocolRequest<Protocol>,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::RegisterProtocolRequest { name, protocol } = request;
        let mut waitlist_hits = vec![];

        let Self {
            timer_api,
            local_subnets,
            protocol_registry,
            protocol_waitlist,
            ..
        } = self;

        let protocol = Arc::new(protocol);

        let mut process_request = || -> p::RegisterProtocolResponse {
            trace!("registering protocol {:?}", name);

            protocol_registry.register_protocol(name.clone(), protocol.clone())?;
            debug!("protocol registered: {:?}", name);

            {
                let Waitlist {
                    entries,
                    by_protocol,
                } = protocol_waitlist;

                while let Some((_, waitlist_key)) = by_protocol
                    .range((name.clone(), None)..)
                    .skip_while(|(_, k)| k.is_none())
                    .take_while(|(n, _)| n == &name)
                    .next()
                {
                    let waitlist_key = waitlist_key.expect("None should have been filtered out");
                    let WaitlistEntry {
                        protocol: protocol_name,
                        reply_to,
                        timer_key,
                    } = entries.remove(waitlist_key).expect("should be present");

                    let timer_key = timer_key.expect("a None should not have been saved");
                    waitlist_hits.push((timer_key, reply_to, protocol.clone()));

                    by_protocol.remove(&(protocol_name, Some(waitlist_key)));
                }
            }

            Ok(())
        };

        let reply_with = process_request();

        for inbound_codec in protocol.inbound_types() {
            let local_type_key = protocol_registry.register_message(inbound_codec);
            for local_subnet in local_subnets.iter().copied().map(NetAddress::from) {
                ctx.tell(
                    ctx.address(),
                    SetRoute {
                        message:     local_type_key,
                        destination: local_subnet,
                        via:         None,
                        metric:      Some(0),
                    },
                )
                .await
                .wrap_err("ctx.tell (when sending SetRoute to self)")?;
            }
        }

        ctx.reply(reply_to, reply_with).await.ok();

        let outbound: Vec<_> = protocol
            .outbound_types()
            .map(|c| (c.name(), protocol_registry.register_message(c)))
            .collect();
        let inbound: Vec<_> = protocol
            .inbound_types()
            .map(|c| (c.name(), protocol_registry.register_message(c)))
            .collect();

        for (timer_key, reply_to, protocol) in waitlist_hits {
            type Ret = p::GetProtocolByNameResponse<Protocol>;

            timer_api
                .cancel(timer_key)
                .await
                .wrap_err("timer_api.cancel")?;

            let outbound = outbound.clone();
            let inbound = inbound.clone();
            ctx.reply(
                reply_to,
                Ret::Ok(p::ProtocolResolved {
                    protocol,
                    outbound,
                    inbound,
                }),
            )
            .await
            .ok();
        }

        Ok(Outcome::NoReply)
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::UnregisterProtocolRequest> for MultinodeManager<Ctx> {
    type Rs = p::UnregisterProtocolResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::UnregisterProtocolRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::UnregisterProtocolRequest { name } = request;
        let process_request = || -> p::UnregisterProtocolResponse {
            trace!("unregistering protocol {:?}", name);

            let Self {
                protocol_registry, ..
            } = self;

            protocol_registry.unregister_protocol(name)
        };

        let reply_with = process_request();
        Ok(Outcome::Reply(reply_with))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::RegisterOpaqueMessageRequest> for MultinodeManager<Ctx> {
    type Rs = p::RegisterOpaqueMessageResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::RegisterOpaqueMessageRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::RegisterOpaqueMessageRequest { name } = request;
        let Self {
            protocol_registry, ..
        } = self;
        let key = protocol_registry.register_message(codec::Opaque(name).into());
        let response = p::RegisterOpaqueMessageResponse { key };
        Ok(Outcome::Reply(response))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::GetMessageNameRequest> for MultinodeManager<Ctx> {
    type Rs = p::GetMessageNameResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::GetMessageNameRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::GetMessageNameRequest { key } = request;
        let Self {
            protocol_registry, ..
        } = self;
        let name = protocol_registry.message_name_by_key(key);
        let response = p::GetMessageNameResponse { name };
        Ok(Outcome::Reply(response))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::GetProtocolByNameRequest> for MultinodeManager<Ctx> {
    type Rs = p::GetProtocolByNameResponse<Protocol>;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        reply_to: RequestHeader,
        request: p::GetProtocolByNameRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::GetProtocolByNameRequest {
            name,
            timeout: timeout_opt,
        } = request;
        let Self {
            timer_api,
            protocol_registry,
            protocol_waitlist,
            ..
        } = self;

        let outcome = match (protocol_registry.protocol_by_name(&name), timeout_opt) {
            (Some(protocol), _) => {
                let outbound = protocol
                    .outbound_types()
                    .map(|c| (c.name(), protocol_registry.register_message(c)))
                    .collect();
                let inbound = protocol
                    .inbound_types()
                    .map(|c| (c.name(), protocol_registry.register_message(c)))
                    .collect();

                Outcome::Reply(Ok(p::ProtocolResolved {
                    protocol,
                    outbound,
                    inbound,
                }))
            },
            (None, None) => {
                Outcome::Reply(Err(ErrorOf::new(
                    p::GetProtocolByNameErrorKind::NoProtocol,
                    "no such protocol",
                )))
            },
            (None, Some(timeout)) => {
                let Waitlist {
                    entries,
                    by_protocol,
                } = protocol_waitlist;
                let waitlist_entry = WaitlistEntry {
                    protocol: name.clone(),
                    reply_to,
                    timer_key: None,
                };
                let waitlist_key = entries.insert(waitlist_entry);
                let timer_key = timer_api
                    .schedule_once_after(timeout, WaitlistTimeoutElapsed { waitlist_key })
                    .await
                    .wrap_err("timer_api.schedule_once")?;
                entries[waitlist_key].timer_key = Some(timer_key);
                let new_key = by_protocol.insert((name, Some(waitlist_key)));
                assert!(new_key);
                Outcome::NoReply
            },
        };

        Ok(outcome)
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::GetLocalSubnetsRequest> for MultinodeManager<Ctx> {
    type Rs = p::GetLocalSubnetsResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::GetLocalSubnetsRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::GetLocalSubnetsRequest = request;
        Ok(Outcome::Reply(
            self.local_subnets
                .iter()
                .copied()
                .map(NetAddress::from)
                .collect::<Vec<_>>(),
        ))
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, p::ResolveTypeIdRequest> for MultinodeManager<Ctx> {
    type Rs = p::ResolveTypeIdResponse;

    async fn on_request(
        &mut self,
        _ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: p::ResolveTypeIdRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let p::ResolveTypeIdRequest { type_id } = request;
        let Self {
            protocol_registry, ..
        } = self;
        let type_key_opt = protocol_registry.local_type_key_by_tid(type_id);
        Ok(Outcome::Reply(p::ResolveTypeIdResponse { type_key_opt }))
    }
}

impl<Ctx: ActorContext> OnMessage<Ctx, WaitlistTimeoutElapsed> for MultinodeManager<Ctx> {
    async fn on_message(
        &mut self,
        ctx: &mut Ctx,
        message: WaitlistTimeoutElapsed,
    ) -> Result<Outcome, AnyError> {
        type Ret = p::GetProtocolByNameResponse<Protocol>;
        let WaitlistTimeoutElapsed { waitlist_key } = message;
        let Self {
            protocol_registry,
            protocol_waitlist,
            ..
        } = self;
        let Waitlist {
            entries,
            by_protocol,
        } = protocol_waitlist;

        let Some(WaitlistEntry {
            protocol, reply_to, ..
        }) = entries.remove(waitlist_key)
        else {
            return Ok(Outcome::NoReply)
        };
        assert!(protocol_registry.protocol_by_name(&protocol).is_none());
        let existing_key = by_protocol.remove(&(protocol, Some(waitlist_key)));
        assert!(existing_key);

        ctx.reply(
            reply_to,
            Ret::Err(ErrorOf::new(
                p::GetProtocolByNameErrorKind::NoProtocol,
                "timed out waiting for protocol",
            )),
        )
        .await
        .ok();

        Ok(Outcome::NoReply)
    }
}

impl<Ctx: ActorContext> OnRequest<Ctx, SubscribeToRoutesRequest> for MultinodeManager<Ctx> {
    type Rs = SubscribeToRoutesResponse;

    async fn on_request(
        &mut self,
        ctx: &mut Ctx,
        _reply_to: RequestHeader,
        request: SubscribeToRoutesRequest,
    ) -> Result<Outcome<Self::Rs>, AnyError> {
        let Self {
            route_registry,
            route_subscribers,
            ..
        } = self;
        let SubscribeToRoutesRequest { deliver_to } = request;

        let routes = route_registry
            .all_routes()
            .map(Into::into)
            .collect::<Vec<_>>();

        let watch_ref = ctx.watch(deliver_to).await;
        route_subscribers.insert(watch_ref, deliver_to);

        trace!(
            "subscribed {} to the route-updates; sending {} routes",
            deliver_to,
            routes.len()
        );

        let response = SubscribeToRoutesResponse { routes };

        Ok(Outcome::Reply(response))
    }
}

impl<Ctx: ActorContext> OnMessage<Ctx, SetRoute> for MultinodeManager<Ctx> {
    async fn on_message(
        &mut self,
        ctx: &mut Ctx,
        set_route: SetRoute,
    ) -> Result<Outcome, AnyError> {
        use std::collections::btree_map::Entry::*;

        let Self {
            route_registry,
            route_subscribers,
            route_gws,
            local_subnets,
            subnet_ingress_sup,
            subnet_ingress_workers,
            ..
        } = self;
        let SetRoute {
            message,
            destination,
            via,
            metric: new_metric,
        } = set_route.clone();

        let contains_net_before = route_registry.contains_net(destination);

        let Ok(old_metric) = route_registry
            .set_route(message, destination, via, new_metric)
            .inspect_err(|reason| error!("error setting route: {}", reason))
        else {
            return Ok(Outcome::NoReply)
        };
        let contains_net_after = route_registry.contains_net(destination);

        match (
            local_subnets.contains(&destination.into()),
            contains_net_before,
            contains_net_after,
        ) {
            (false, true, false) => {
                debug!("stopping subnet_ingress [destination: {}]", destination);

                let Occupied(subnet_ingress_entry) =
                    subnet_ingress_workers.entry(destination.into())
                else {
                    panic!("subnet_ingress is not present: {}", destination);
                };
                let subnet_ingress_worker = *subnet_ingress_entry.get();
                let () = ctx
                    .ask::<_, uni_sup::StopResponse>(
                        *subnet_ingress_sup,
                        uni_sup::StopRequest {
                            child: subnet_ingress_worker,
                        },
                        Duration::from_secs(10),
                    )
                    .await
                    .wrap_err("ask")?
                    .wrap_err("uni_sup::StopResponse")?;
                subnet_ingress_entry.remove();

                info!(
                    "subnet_ingress stopped [destination: {}; worker: {}]",
                    destination, subnet_ingress_worker
                );
            },
            (false, false, true) => {
                debug!("starting subnet_ingress [destination: {}]", destination);
                let Vacant(subnet_ingress_entry) = subnet_ingress_workers.entry(destination.into())
                else {
                    panic!("duplicate subnet_ingress worker: {}", destination)
                };
                let subnet_ingress_worker = ctx
                    .ask::<_, uni_sup::StartResponse>(
                        *subnet_ingress_sup,
                        uni_sup::StartRequest {
                            args: (destination,),
                        },
                        Duration::from_secs(1),
                    )
                    .await
                    .wrap_err("ask")?
                    .wrap_err("uni_sup::StartResponse")?;
                subnet_ingress_entry.insert(subnet_ingress_worker);

                info!(
                    "subnet_ingress started [destination: {}; worker: {}]",
                    destination, subnet_ingress_worker
                );
            },
            (false, ..) => {},
            (true, ..) => {},
        }

        debug!(
            "route updated [msg: {:?}; dst: {} gw: {}; metric: {:?} -> {:?}]",
            message,
            destination,
            via.map(|n| n.to_string()).unwrap_or_default(),
            old_metric,
            new_metric
        );

        if let Some(gw) = via
            && !route_gws.contains_right(&gw)
        {
            debug!("watching after gw {}", gw);
            let watch_ref = ctx.watch(gw).await;
            route_gws.insert(watch_ref, gw);
        }

        for subscriber in route_subscribers.values().copied() {
            trace!("announcing route update to {}", subscriber);
            let _ = ctx.tell(subscriber, set_route.clone()).await;
        }

        Ok(Outcome::NoReply)
    }
}

impl<Ctx: ActorContext> OnMessage<Ctx, sys::Down> for MultinodeManager<Ctx> {
    async fn on_message(&mut self, ctx: &mut Ctx, message: sys::Down) -> Result<Outcome, AnyError> {
        let sys::Down {
            watch_ref, peer, ..
        } = message;
        let Self {
            route_gws,
            route_registry,
            route_subscribers,
            ..
        } = self;

        if route_subscribers.contains_key(&watch_ref) {
            debug!("sys::Down: removing route-subscriber {}", peer);
            route_subscribers.remove(&watch_ref);
        }

        if route_gws.contains_left(&watch_ref) {
            let gw = peer;
            debug!("sys::Down: removing route-gw {}", gw);
            route_gws.remove_by_left(&watch_ref);
            for (message, destination, _metric) in route_registry.all_routes_by_gw(gw) {
                trace!(
                    "- sys::Down: removing route [msg: {:?}, dst: {}, via: {}]",
                    message, destination, gw
                );
                let set_route = SetRoute {
                    message,
                    destination,
                    via: Some(gw),
                    metric: None,
                };
                let _ = ctx.tell(ctx.address(), set_route).await;
            }
        }
        Ok(Outcome::NoReply)
    }
}

async fn start_subnet_ingress_sup<Ctx>(ctx: &mut Ctx) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
{
    let multinode_manager = ctx.address();
    let launcher = ActorFactoryMut::new(move |(net_address,): (NetAddress,)| {
        local::boxed_from_fn((
            crate::actors::subnet_ingress::run,
            (multinode_manager, net_address),
        ))
    });
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));
    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let subnet_ingress_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(subnet_ingress_sup)
}

async fn start_connection_sup<Ctx, IO>(
    ctx: &mut Ctx,
    multinode_manager: Address,
) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
    IO: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    let launcher = ActorFactoryMut::new(
        move |(io, options, protocol): (
            IO,
            Arc<nm::Options>,
            Arc<[p::ProtocolResolved<Protocol>]>,
        )| {
            local::boxed_from_fn((
                crate::actors::iostream_connection::run,
                (multinode_manager, io, options, protocol),
            ))
        },
    );
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));
    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let connection_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(connection_sup)
}

async fn start_uds_acceptor_sup<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
{
    let launcher = ActorFactoryMut::new(
        move |(bind_addr, protocol_names, options): (
            Box<Path>,
            Vec<nm::ProtocolName>,
            nm::Options,
        )| {
            local::boxed_from_fn((
                crate::actors::uds_acceptor::run,
                (connection_sup, bind_addr, protocol_names, options),
            ))
        },
    );
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));

    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let acceptor_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(acceptor_sup)
}

async fn start_tcp_connector_sup<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
{
    let launcher = ActorFactoryMut::new(
        move |(destination_addr, protocol_names, options): (
            SocketAddr,
            Vec<nm::ProtocolName>,
            nm::Options,
        )| {
            local::boxed_from_fn((
                crate::actors::tcp_connector::run,
                (connection_sup, destination_addr, protocol_names, options),
            ))
        },
    );
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));
    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let connector_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(connector_sup)
}

async fn start_uds_connector_sup<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
{
    let launcher = ActorFactoryMut::new(
        move |(destination_addr, protocol_names, options): (
            Box<Path>,
            Vec<nm::ProtocolName>,
            nm::Options,
        )| {
            local::boxed_from_fn((
                crate::actors::uds_connector::run,
                (connection_sup, destination_addr, protocol_names, options),
            ))
        },
    );
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));
    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let connector_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(connector_sup)
}

async fn start_tcp_acceptor_sup<Ctx>(
    ctx: &mut Ctx,
    connection_sup: Address,
) -> Result<Address, AnyError>
where
    Ctx: ActorContext,
{
    let launcher = ActorFactoryMut::new(
        move |(bind_addr, protocol_names, options): (
            SocketAddr,
            Vec<nm::ProtocolName>,
            nm::Options,
        )| {
            local::boxed_from_fn((
                crate::actors::tcp_acceptor::run,
                (connection_sup, bind_addr, protocol_names, options),
            ))
        },
    );
    let child_spec = ChildSpec::new(launcher)
        .with_child_type(child_type::Temporary)
        .with_init_type(InitType::WithAck {
            start_timeout: Duration::from_secs(5),
        })
        .with_stop_timeout(Duration::from_secs(10));
    let sup_spec = UniformSup::new(child_spec);
    let sup_runnable = local::boxed_from_fn((mm1_sup::uniform::uniform_sup, (sup_spec,)));
    let acceptor_sup = ctx
        .start(sup_runnable, true, Duration::from_secs(1))
        .await
        .wrap_err("ctx.start")?;

    Ok(acceptor_sup)
}
