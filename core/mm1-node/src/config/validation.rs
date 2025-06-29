use std::collections::BTreeSet;
use std::ops::Deref;

use mm1_address::address_range::AddressRange;
use mm1_address::subnet::NetAddress;
use scc::HashSet;

use crate::config::{DeclareSubnet, Mm1NodeConfig, SubnetKind};

#[derive(Debug, Clone)]
pub struct Valid<C>(C);

#[derive(Debug, thiserror::Error)]
#[error("validation error:\n{}", errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n-"))]
pub struct ValidationError<T> {
    pub errors:   Vec<String>,
    pub rejected: Box<T>,
}

impl Mm1NodeConfig {
    pub fn validate(self) -> Result<Valid<Self>, ValidationError<Self>> {
        TryFrom::try_from(self)
    }
}

impl TryFrom<Mm1NodeConfig> for Valid<Mm1NodeConfig> {
    type Error = ValidationError<Mm1NodeConfig>;

    fn try_from(node_config: Mm1NodeConfig) -> Result<Self, Self::Error> {
        let errors = {
            let known_codecs = node_config.codecs.iter().map(|c| c.name.as_str()).collect();

            let runtime_keys = node_config.runtime.runtime_keys().collect();
            validate_subnets(&node_config.subnets, &known_codecs)
                .chain(
                    node_config
                        .actor
                        .ensure_runtime_keys_are_valid(&runtime_keys)
                        .err(),
                )
                .collect::<Vec<_>>()
        };

        if errors.is_empty() {
            Ok(Self(node_config))
        } else {
            Err(ValidationError {
                errors,
                rejected: Box::new(node_config),
            })
        }
    }
}

fn validate_subnets<'a>(
    subnets: impl IntoIterator<Item = &'a DeclareSubnet>,
    #[allow(unused_variables)] known_codecs: &HashSet<&str>,
) -> impl Iterator<Item = String> {
    let mut errors: Vec<String> = Default::default();
    let mut local_subnet: Option<NetAddress> = Default::default();
    let mut subnets_tree: BTreeSet<AddressRange> = Default::default();

    for subnet in subnets {
        let net_address = subnet.net_address;
        let address_range = AddressRange::from(net_address);
        if let Some(existing) = subnets_tree.get(&address_range).copied() {
            errors.push(format!(
                "conflicting subnets: {} vs {}",
                NetAddress::from(existing),
                net_address
            ));
        } else {
            subnets_tree.insert(address_range);
        }

        match (local_subnet, &subnet.kind) {
            (None, SubnetKind::Local) => {
                local_subnet = Some(subnet.net_address);
            },
            (Some(existing), SubnetKind::Local) => {
                errors.push(format!(
                    "a subnet of type 'local' defined more than once: at least {existing} and \
                     {net_address}"
                ));
            },
            #[cfg(feature = "multinode")]
            (_, SubnetKind::Remote(remote)) => {
                if !known_codecs.contains(remote.codec.as_str()) {
                    errors.push(format!(
                        "subnet {} referes unknown codec {:?}",
                        net_address, remote.codec
                    ));
                }
            },
        }
    }

    if local_subnet.is_none() {
        errors.push("exactly one subnet of type 'local' must be defined".into());
    }

    errors.into_iter()
}

impl<C> Deref for Valid<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::config::validation::Valid;

    #[test]
    fn ergonomics() {
        struct Config {
            field: u32,
        }
        impl Config {
            fn method(&self) -> u32 {
                self.field
            }
        }

        let valid_value = Valid(Config { field: 42 });
        assert_eq!(valid_value.field, 42);
        assert_eq!(valid_value.method(), 42);

        let valid_reference = Valid(&Config { field: 13 });
        assert_eq!(valid_reference.field, 13);
        assert_eq!(valid_reference.method(), 13);
    }
}
