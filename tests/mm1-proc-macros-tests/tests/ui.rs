//! `trybuild` harness for the `mm1-proc-macros` macros.
//!
//! `pass/` holds programs that must compile. As macro bugs get fixed, add
//! `compile_fail` cases under `fail/` (e.g. the `dispatch!` footguns in #155
//! and the dead `#[derive(Traversable)]` in #139).

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    // t.compile_fail("tests/ui/fail/*.rs");
}
