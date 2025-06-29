use mm1_common::errors::error_of::ErrorOf;
use mm1_proto_network_management::{RegisterSubnetErrorKind, RegisterSubnetResponse};

use crate::network_manager::*;

pub(super) async fn process_request<C>(
    state: &mut State<C>,
    request: RegisterSubnetRequest,
) -> Result<RegisterSubnetResponse, AnyError>
where
    C: serde::de::DeserializeOwned,
{
    use std::collections::btree_map::Entry::*;

    let RegisterSubnetRequest {
        net_address,
        config,
    } = request;
    let subnet_config = match serde_json::from_value::<SerializedSubnetConfig<C>>(config) {
        Ok(config) => config,
        Err(reason) => {
            return Ok(Err(ErrorOf::new(
                RegisterSubnetErrorKind::BadRequest,
                format!("parse error: {reason}"),
            )))
        },
    };

    let vacant_entry = match state.subnets.entry(net_address.into()) {
        Vacant(v) => v,
        Occupied(o) => {
            return Ok(Err(ErrorOf::new(
                RegisterSubnetErrorKind::Conflict,
                format!("conflict with {}", NetAddress::from(*o.key())),
            )))
        },
    };

    let subnet_state = match subnet_config {
        SerializedSubnetConfig::Local => SubnetState::Local,
        SerializedSubnetConfig::Remote(config) => {
            SubnetState::Remote(RemoteSubnetState::new(config))
        },
    };
    vacant_entry.insert(subnet_state);

    Ok(Ok(()))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "props")]
pub(crate) enum SerializedSubnetConfig<C> {
    Local,
    Remote(C),
}
