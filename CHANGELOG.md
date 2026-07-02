# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/). While
the crate is pre-1.0, breaking changes are batched into `0.MINOR.0` bumps and
plain bug fixes ship as `0.7.PATCH`.

## [Unreleased]

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
