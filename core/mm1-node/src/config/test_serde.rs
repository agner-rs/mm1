#[allow(unused_imports)]
use test_case::test_case;

#[allow(unused_imports)]
use crate::config::*;

#[test_case(
    DeclareSubnet {
        net_address: "<cafe:>/16".parse().unwrap(),
        kind: SubnetKind::Local,
    },
    r#"
        type: local
        net_address: <cafe:>/16
    "#
    ; "delare local subnet"
)]
#[cfg(feature = "multinode")]
#[test_case(
    DeclareSubnet {
        net_address: "<cafe:>/16".parse().unwrap(),
        kind: SubnetKind::Remote(RemoteSubnetConfig {
            codec: "codec-name".into(),
            opaque: Default::default(),
        }),
    },
    r#"
        type: remote
        net_address: <cafe:>/16
        codec: codec-name
    "#
    ; "declare remote subnet"
)]
#[test_case(
    Mm1NodeConfig {
        subnets: vec![],
        ..Default::default()
    },
    r#"
        subnets: []
    "#
    ; "a config with empty subnets"
)]
#[test_case(
    Mm1NodeConfig {
        subnets: vec![
            DeclareSubnet {
                net_address: "<aaaa:>/16".parse().unwrap(),
                kind: SubnetKind::Local,
            },
            DeclareSubnet {
                net_address: "<bbbb:>/16".parse().unwrap(),
                kind: SubnetKind::Remote(RemoteSubnetConfig {
                    codec: "worker-node:worker-node".into(),
                    opaque: Default::default(),
                }),
            },
            DeclareSubnet {
                net_address: "<cafe:>/16".parse().unwrap(),
                kind: SubnetKind::Remote(RemoteSubnetConfig {
                    codec: "worker-node:coordinator-node".into(),
                    opaque: Default::default(),
                }),
            },
        ],
        ..Default::default()
    },
    r#"
        subnets: 
         - type: local
           net_address: <aaaa:>/16
         - type: remote
           net_address: <bbbb:>/16
           codec: worker-node:worker-node
         - type: remote
           net_address: <cafe:>/16
           codec: worker-node:coordinator-node
    "#
    ; "a config with three subnets"
)]
fn should_deserialize<C: serde::de::DeserializeOwned + std::fmt::Debug>(expected: C, yaml: &str) {
    assert_eq!(
        format!("{:?}", serde_yaml::from_str::<C>(yaml).unwrap()),
        format!("{:?}", expected),
    )
}
