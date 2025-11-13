use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;
use mm1_proto::message;
use mm1_proto_network_management::protocols::LocalTypeKey;

use crate::common::RouteMetric;

#[message(base_path = ::mm1_proto)]
pub struct SubscribeToRoutesRequest {
    pub deliver_to: Address,
}

#[message(base_path = ::mm1_proto)]
pub struct SubscribeToRoutesResponse {
    pub routes: Vec<SetRoute>,
}

#[message(base_path = ::mm1_proto)]
pub struct Forward {
    pub local_type_key: LocalTypeKey,
    pub body:           Box<[u8]>,
}

#[derive(Clone)]
#[message(base_path = ::mm1_proto)]
pub struct SetRoute {
    pub message:     LocalTypeKey,
    pub destination: NetAddress,
    pub via:         Option<Address>,
    pub metric:      Option<RouteMetric>,
}

impl From<(LocalTypeKey, NetAddress, Option<Address>, RouteMetric)> for SetRoute {
    fn from(
        (message, destination, via, metric): (
            LocalTypeKey,
            NetAddress,
            Option<Address>,
            RouteMetric,
        ),
    ) -> Self {
        Self {
            message,
            destination,
            via,
            metric: Some(metric),
        }
    }
}
