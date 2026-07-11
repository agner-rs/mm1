# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/). While
the crate is pre-1.0, breaking changes are batched into `0.MINOR.0` bumps and
plain bug fixes ship as `0.7.PATCH`.

## [Unreleased]

## [0.7.26] - 2026-07-11

Runtime lifecycle and polish. Bug fixes only; no breaking API changes.

### Fixed
- Container teardown tolerates a late `ForkDone` whose `ForkAdded` call was not
  applied before shutdown, so the restored mixed-supervisor test no longer
  panics or hangs.
- Node configuration rejects the default actor runtime, or a selected named
  actor runtime, when its Tokio time driver is disabled. Unused named runtimes
  remain valid.
- `Rt::run` tracks spawned containers on default and named executors, closes
  admission when the main run ends, and makes a bounded, best-effort attempt at
  normal cleanup before `Kill` and task-abort fallbacks. Tokio cancellation is
  cooperative: blocking or non-yielding actor code may still outlive `Rt::run`.
- The test runtime returns already queued standalone-context queries before
  reporting that its task set is empty.
- The test runtime applies an actor's returned `ActorExit`, so `Quit` is
  observed before `Done`, matching production execution.
- A `WatchRef` transferred to another fork can be cancelled there without
  leaking the original watch or producing a stray `Down`.
- Address pools normalize noncanonical input network addresses before building
  their allocation trie.
- Non-string panic payloads produce the useful
  `<non-string panic payload>` diagnostic instead of an empty message.
- A uniform supervisor that exceeds a child's restart intensity attempts to
  reap every remaining started child before returning the original intensity
  error; one cleanup failure no longer skips later survivors or replaces that
  error.

### Changed
- `dispatch!` accepts a plain `_` catch-all and reports focused errors for bare
  binding catch-alls, guarded catch-alls, and unsupported patterns. Existing
  unit-struct patterns and borrowed typed guards remain supported.
- The private node registry now uses `scc` 3.8.4, removing the resolved
  `RUSTSEC-2026-0205` advisory exception.

## [0.7.25] - 2026-07-10

Supervisor robustness. Bug fixes only; no breaking API changes.

### Fixed
- The mixed supervisor no longer crashes when its own inbox is full: child
  reports (`Started` / `StartFailed` / `StopFailed`) are sent on the priority
  lane instead of a regular send that could return `Full` and panic the
  supervisor.
- The uniform supervisor's child-start report is likewise sent on the priority
  lane, so a full supervisor inbox can no longer drop it and strand a started
  child (which previously drove the supervisor to give up on live children).
- A uniform-supervisor child that fails to start no longer leaks its `Starting`
  slot: the failed start is reported and the slot is dropped.

## [0.7.24] - 2026-07-09

Core-runtime robustness. Bug fixes only; no breaking API changes.

### Fixed
- `dispatch!` no longer panics on a message that no arm matched. Any peer that
  learns an address could previously crash an actor that lacked a catch-all;
  the unmatched message is now logged and dropped.
- `mm1-ask` checks the response id, so a late reply to a timed-out `ask` is never
  returned as the answer to a later `ask`; and a message that is not the response
  is put back for the actor's normal receive loop instead of being destroyed.
- `ctx.fork()` is cancellation-safe: a `fork()` future dropped at its await (e.g.
  in a `timeout` or `select!`) no longer leaks a fork entry, and a later fork
  that reuses the address no longer panics.
- A fork's or actor's outstanding watches are torn down when it exits: the
  watched targets are sent `Unwatch`, so their `watched_by` no longer grows
  without bound and they no longer send a stray `Down` to a recycled address.

### Behavior changes to note
- **`dispatch!`:** an unmatched message is now logged at `WARN` and dropped
  rather than panicking. A `dispatch!` used in value position must supply its own
  catch-all arm (the fallback now evaluates to `()`).
- **`mm1-ask`:** an `ask` no longer fails when a non-response message arrives
  while it waits; it keeps waiting until the deadline. A stale response for an
  earlier, timed-out request is discarded rather than returned.
- **Links:** a fork's normal end now delivers `Exited { normal_exit: true }` to a
  *trapping* linked peer, matching the `Down` a watcher already receives (an
  untrapped peer is unaffected). An abnormal end is still reported as
  `normal_exit: false` by the actor-exit path.

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
