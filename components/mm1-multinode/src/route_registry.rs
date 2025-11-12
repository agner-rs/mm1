use std::collections::btree_map::Entry::{Occupied as BO, Vacant as BV};
use std::collections::{BTreeMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use mm1_address::address::Address;
use mm1_address::address_range::AddressRange;
use mm1_address::subnet::NetAddress;
use mm1_common::log::trace;
use mm1_proto_network_management::protocols::LocalTypeKey;
use slotmap::SlotMap;

use crate::common::RouteMetric;

const MAX_CANDIDATES: usize = 4;

#[derive(Debug, Default)]
pub struct RouteRegistry {
    route_entries: SlotMap<RouteKey, RouteEntry>,
    by_gw:         BTreeMap<(Option<Address>, AddressRange, LocalTypeKey), RouteKey>,
    by_dst:        BTreeMap<(AddressRange, LocalTypeKey, RouteMetric), HashSet<RouteKey>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SetRouteError {
    #[error("conflict with {}", _0)]
    Conflict(NetAddress),
}

#[derive(Debug, thiserror::Error)]
pub enum FindRouteError {
    #[error("no route")]
    NoRoute,
}

impl RouteRegistry {
    pub fn all_routes(
        &self,
    ) -> impl Iterator<Item = (LocalTypeKey, NetAddress, Option<Address>, RouteMetric)> + '_ {
        self.route_entries.iter().map(
            |(
                _,
                RouteEntry {
                    message,
                    dst_net,
                    gw,
                    metric,
                },
            )| (*message, *dst_net, *gw, *metric),
        )
    }

    pub fn contains_net(&self, net_address: NetAddress) -> bool {
        let Self { by_dst, .. } = self;
        let probe_key = (
            AddressRange::from(net_address),
            LocalTypeKey::default(),
            RouteMetric::default(),
        );

        by_dst
            .range(..probe_key)
            .next_back()
            .is_some_and(|((range, ..), _)| NetAddress::from(*range) == net_address)
            || by_dst
                .range(probe_key..)
                .next()
                .is_some_and(|((range, ..), _)| NetAddress::from(*range) == net_address)
    }

    pub fn all_routes_by_gw(
        &self,
        sought_gw: Address,
    ) -> impl Iterator<Item = (LocalTypeKey, NetAddress, RouteMetric)> + '_ {
        let Self { route_entries, .. } = self;

        route_entries.iter().filter_map(move |(_key, entry)| {
            let RouteEntry {
                message,
                dst_net,
                gw,
                metric,
            } = entry;
            gw.is_some_and(|gw| gw == sought_gw)
                .then_some((*message, *dst_net, *metric))
        })
    }

    pub fn find_route(
        &self,
        message: LocalTypeKey,
        dst_addr: Address,
    ) -> Result<(Option<Address>, RouteMetric), FindRouteError> {
        let Self {
            route_entries,
            by_dst: by_destination,
            ..
        } = self;
        let lookup_key = (AddressRange::from(dst_addr), message, 0);
        let candidates: Vec<_> = by_destination
            .range(lookup_key..)
            .next()
            .filter(|((_, m, _), _)| *m == message)
            .ok_or(FindRouteError::NoRoute)?
            .1
            .iter()
            .take(MAX_CANDIDATES)
            .copied()
            .collect();
        assert!(!candidates.is_empty());

        let addr_hash = {
            let mut h = DefaultHasher::new();
            dst_addr.hash(&mut h);
            h.finish() as usize
        };

        let selected_key = candidates[addr_hash % candidates.len()];
        let selected_route = &route_entries[selected_key];

        Ok((selected_route.gw, selected_route.metric))
    }

    pub fn set_route(
        &mut self,
        message: LocalTypeKey,
        dst_net: NetAddress,
        gw: Option<Address>,
        set_metric: Option<RouteMetric>,
    ) -> Result<Option<RouteMetric>, SetRouteError> {
        trace!(
            "RouteRegistry::set_route [msg: {:?}; dst: {}; gw: {}; metric: {:?}]",
            message,
            dst_net,
            gw.map(|a| a.to_string()).unwrap_or_default(),
            set_metric
        );

        let Self {
            route_entries,
            by_dst,
            by_gw,
        } = self;

        let new_by_destination = set_metric
            .map(|new_metric| {
                let entry = by_dst.entry((dst_net.into(), message, new_metric));
                let existing_address = NetAddress::from(entry.key().0);
                if existing_address != dst_net {
                    Err(SetRouteError::Conflict(existing_address))
                } else {
                    Ok(entry)
                }
            })
            .transpose()?;

        let by_gw = by_gw.entry((gw, dst_net.into(), message));
        let existing_address = NetAddress::from(by_gw.key().1);
        if existing_address != dst_net {
            return Err(SetRouteError::Conflict(existing_address))
        }

        match (new_by_destination, by_gw) {
            (None, BV(_)) => Ok(None),
            (Some(new_by_dst), BO(by_gw_and_message)) => {
                let key = *by_gw_and_message.get();
                let new_metric = new_by_dst.key().2;
                let old_metric = route_entries[key].metric;

                if new_metric == old_metric {
                    return Ok(Some(old_metric))
                }

                route_entries[key].metric = new_metric;

                new_by_dst.or_default().insert(key);

                let BO(mut old_by_destination) =
                    by_dst.entry((dst_net.into(), message, old_metric))
                else {
                    panic!("old_by_destination missing")
                };
                let existed_before = old_by_destination.get_mut().remove(&key);
                assert!(existed_before);

                if old_by_destination.get().is_empty() {
                    old_by_destination.remove();
                }

                Ok(Some(old_metric))
            },
            (Some(new_by_dst), BV(by_gw)) => {
                let new_metric = new_by_dst.key().2;
                let entry = RouteEntry {
                    message,
                    dst_net,
                    gw,
                    metric: new_metric,
                };
                let key = route_entries.insert(entry);

                by_gw.insert(key);
                let newly_added = new_by_dst.or_default().insert(key);
                assert!(newly_added);

                Ok(None)
            },

            (None, BO(by_gw)) => {
                let key = *by_gw.get();
                let old_metric = route_entries[key].metric;

                let BO(mut old_by_dst) = by_dst.entry((dst_net.into(), message, old_metric)) else {
                    panic!("old_by_destination missing")
                };
                let existed_before = old_by_dst.get_mut().remove(&key);
                assert!(existed_before);

                if old_by_dst.get().is_empty() {
                    old_by_dst.remove();
                }
                by_gw.remove();
                route_entries.remove(key);

                Ok(Some(old_metric))
            },
        }
    }
}

slotmap::new_key_type! {
    struct RouteKey;
}

#[derive(Debug)]
struct RouteEntry {
    message: LocalTypeKey,
    dst_net: NetAddress,
    gw:      Option<Address>,
    metric:  RouteMetric,
}

#[cfg(test)]
mod tests {
    use mm1_proto_network_management::protocols::LocalTypeKey;

    use super::*;

    #[test]
    fn routes() {
        let mut routes = RouteRegistry::default();

        assert!(
            routes
                .set_route(m(), n("<f:100>/56"), Some(a("<a:101>")), Some(1))
                .unwrap()
                .is_none()
        );
        routes
            .set_route(m(), n("<f:100>/60"), Some(a("<a:102>")), Some(1))
            .unwrap_err();
        assert!(
            routes
                .set_route(m(), n("<f:100>/56"), Some(a("<a:102>")), Some(2))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            routes.find_route(m(), a("<f:100>")).unwrap(),
            (Some(a("<a:101>")), 1)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:101>")).unwrap(),
            (Some(a("<a:101>")), 1)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:102>")).unwrap(),
            (Some(a("<a:101>")), 1)
        );

        assert_eq!(
            routes
                .set_route(m(), n("<f:100>/56"), Some(a("<a:101>")), Some(3))
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:100>")).unwrap(),
            (Some(a("<a:102>")), 2)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:101>")).unwrap(),
            (Some(a("<a:102>")), 2)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:102>")).unwrap(),
            (Some(a("<a:102>")), 2)
        );

        assert_eq!(
            routes
                .set_route(m(), n("<f:100>/56"), Some(a("<a:102>")), None)
                .unwrap(),
            Some(2)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:100>")).unwrap(),
            (Some(a("<a:101>")), 3)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:101>")).unwrap(),
            (Some(a("<a:101>")), 3)
        );
        assert_eq!(
            routes.find_route(m(), a("<f:102>")).unwrap(),
            (Some(a("<a:101>")), 3)
        );

        assert_eq!(
            routes
                .set_route(m(), n("<f:100>/56"), Some(a("<a:101>")), None)
                .unwrap(),
            Some(3)
        );
        routes.find_route(m(), a("<f:100>")).unwrap_err();
    }

    #[test]
    fn reproduce_panic() {
        let mut r = RouteRegistry::default();

        r.set_route(m(), n("<aa:>/16"), Some(a("<bb:801>")), Some(1))
            .unwrap();
        r.set_route(m(), n("<bb:>/16"), Some(a("<bb:801>")), Some(2))
            .unwrap();
    }

    fn a(a: &str) -> Address {
        a.parse().unwrap()
    }
    fn n(n: &str) -> NetAddress {
        n.parse().unwrap()
    }
    fn m() -> LocalTypeKey {
        LocalTypeKey::default()
    }
}
