#![cfg(feature = "multinode")]

use std::collections::HashSet;

use insta::assert_yaml_snapshot;
use test_case::test_case;

use crate::config::*;

#[test_case(
    r#"
    "#
    ; "the default config is valid"
)]
#[test_case(
    r#"
    subnets:
        - net_address: <cafe:>/16
          type: local
    "#
    ; "single local subnet"
)]
#[test_case(
    r#"
    subnets:
        - net_address: <cafe:>/16
          type: local
        - net_address: <beef:>/16
          type: local
    "#
    ; "duplicate local subnet"
)]
#[test_case(
    r#"
    codecs:
        - name: some-codec
    subnets:
        - net_address: <cafe:>/16
          type: local
        - net_address: <beef:>/16
          type: remote
          codec: some-codec
    "#
    ; "two distinct subnets, one of them — local"
)]
#[test_case(
    r#"
    codecs:
        - name: some-codec
    subnets:
        - net_address: <cafe:>/16
          type: local
        - net_address: <cafe:>/16
          type: remote
          codec: some-codec
    "#
    ; "two subnets, same net-address"
)]
#[test_case(
    r#"
    codecs:
        - name: some-codec
    subnets:
        - net_address: <cafe:>/16
          type: local
        - net_address: <cafeffff:>/32
          type: remote
          codec: some-codec
    "#
    ; "two subnets, overlapping net-address"
)]
fn test_validation(yaml: &str) {
    let actual_errors = serde_yaml::from_str::<Mm1NodeConfig>(yaml)
        .expect("invalid format")
        .validate()
        .err()
        .into_iter()
        .flat_map(|e| e.errors)
        .collect::<HashSet<_>>();

    let thread_name = std::thread::current()
        .name()
        .map(ToOwned::to_owned)
        .unwrap();
    assert_yaml_snapshot!(thread_name, actual_errors);
}
