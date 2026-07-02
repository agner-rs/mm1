//! Compile-time regression guard for the facade re-export gaps in #159.
//!
//! Each `use` below names a public item that the `mm1` facade should re-export.
//! Before #159 several of these were unreachable through `mm1` (most visibly
//! `mm1::server::Server`, the return type of `mm1::server::new()`). This test
//! only needs to *compile*; naming the items is the assertion.

#![allow(unused_imports)]

// Address-range type (interval-tree keys), previously not re-exported.
use mm1::address::AddressRange;
// The actor-exit trait, previously not re-exported.
use mm1::core::actor_exit::ActorExit;
// Futures-metrics helpers, previously not re-exported.
use mm1::common::metrics::{MeasuredFuture, Metrics};

#[cfg(feature = "server")]
// The server builder type + its type-list helpers; `mm1::server::new()` used to
// return an unnameable `Server<Ctx, (), ()>`.
use mm1::server::{AppendMsg, AppendReq, Server};

#[cfg(feature = "runtime")]
// The context type referenced by the exported `runtime::Local` alias.
use mm1::runtime::ActorContext;

#[cfg(feature = "logger")]
use mm1::logger as _logger;

#[cfg(feature = "name-service")]
use mm1::name_service::{self, proto as _named_proto};

#[test]
fn facade_surface_is_nameable() {
    // Compiling the imports above is the whole test.
}
