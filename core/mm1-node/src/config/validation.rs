use std::collections::BTreeSet;
use std::ops::Deref;

use mm1_address::address_range::AddressRange;
use mm1_address::subnet::NetAddress;

use crate::config::{LocalSubnetKind, Mm1NodeConfig};

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
        let runtime_keys = node_config.runtime.runtime_keys().collect();

        let runtime_keys_validation_error_opt = node_config
            .actor
            .ensure_runtime_keys_are_valid(&runtime_keys)
            .err();

        let exactly_one_auto_network_error_opt = (node_config
            .local_subnets
            .iter()
            .filter(|d| matches!(d.kind, LocalSubnetKind::Auto))
            .count()
            != 1)
            .then_some("exactly one local-subnet with kind:auto must be defined".into());

        let overlapping_local_nets = {
            let mut nets = BTreeSet::<AddressRange>::new();
            let mut conflicts = vec![];

            for this in node_config.local_subnets.iter().map(|d| d.net) {
                if let Some(that) = nets
                    .get(&AddressRange::from(this))
                    .map(|r| NetAddress::from(*r))
                {
                    conflicts.push(format!("{} overlaps with {}", this, that));
                } else {
                    nets.insert(this.into());
                }
            }
            conflicts
        };

        let errors = runtime_keys_validation_error_opt
            .into_iter()
            .chain(exactly_one_auto_network_error_opt)
            .chain(overlapping_local_nets)
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(Valid(node_config))
        } else {
            Err(ValidationError {
                errors,
                rejected: Box::new(node_config),
            })
        }
    }
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
