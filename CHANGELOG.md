# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/). While
the crate is pre-1.0, breaking changes are batched into `0.MINOR.0` bumps and
plain bug fixes ship as `0.7.PATCH`.

## [Unreleased]

## [0.7.23] - 2026-07-02

Bug-fix release: two critical fixes plus a batch of isolated fixes. No breaking
API changes.

### Fixed
- **Critical:** the mixed supervisor no longer restarts a `Temporary` child (any
  exit) or a `Transient` child (normal exit); such children are left stopped.
- **Critical:** `mm1_common::serde::binary::from_hex` decodes correctly (the
  length check was inverted, rejecting valid input and panicking on odd input).
- `NetMask` deserialization now rejects out-of-range masks instead of accepting
  them and panicking later — including over the wire via `NetAddress`.
- The address pool coalesces free space across its internal tries, so a lease no
  longer fails while the space is entirely free.
- The init actor only shuts the node down on the *main* actor's exit; an
  auxiliary service exiting is logged and ignored.
- The name service enforces registration expiry: a dead exclusive owner no
  longer blocks its name, and expired entries are swept on registration.
- `MeasuredFuture` now records a non-zero `wait_time` (its `last_poll` was never
  written).

### Changed
- The address-range `Ord` invariant (`lo <= hi`) is enforced at construction and
  documented; the assert was removed from the hot comparison path.
- Removed the dead `#[derive(Traversable)]` proc-macro (it never compiled and had
  no users).

### Behavior changes to note
- **Logging:** an empty `log_target_filter` now defers to `min_log_level` instead
  of dropping every event, a target statement now applies to child targets
  (`a=debug` covers `a::b`), and `LogTargetConfig` round-trips (its `Display` used
  `*` instead of `::`). Configs that worked around the old behavior may now log
  more than before.
- **Addresses:** an invalid net-mask on a wire message is now rejected at
  deserialize time rather than surfacing as a later panic.

## [0.7.22] - 2026-07-02

This is the first release with a curated changelog. It groups the repository's
"foundation" work: CI, docs, dependency, and test-scaffolding changes. It
contains no breaking API changes.

### Added
- `README.md`, this `CHANGELOG.md`, and real crates.io metadata (description,
  keywords, categories).
- CI: a docs job (`cargo doc` with `-D warnings` + doctests), so broken doctests
  and intra-doc links now fail CI.
- Facade re-exports that were previously unreachable: `server::{Server,
  AppendReq, AppendMsg}`, `runtime::ActorContext`, `address::AddressRange`,
  `core::actor_exit`, and `common::metrics`.
- New optional facade features: `logger` (exposes `mm1::logger`) and
  `name-service` (exposes `mm1::name_service`).
- A `trybuild` UI-test harness for the procedural macros.
- Declared MSRV: Rust 1.91 (`rust-version`).

### Changed
- The `multinode` feature now implies `runtime`, so a node is usable through the
  facade with `--features multinode` alone.
- Replaced unmaintained dependencies: `structopt` → `clap` v4,
  `serde_yaml` → `serde_yaml_ng`, `fs2` → `fs4`.
- Pinned the CI toolchains (clippy + tests on the MSRV; rustfmt on a pinned
  nightly) to stop silent drift.

### Fixed
- The `#[message]` doctest in `mm1` now compiles and runs; removed a dangling
  `crate::core::context::Call` intra-doc link.
- Dependabot configuration (it previously had an empty ecosystem and did
  nothing).

### Removed
- The dead `tests/mm1-tests` crate.

---

Releases before this changelog was introduced (up to `0.7.21`) are recorded in
the git history and in the `chore: bump ...` commits.
