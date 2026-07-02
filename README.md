# mm1

An Erlang-style actor runtime for Rust.

`mm1` gives you lightweight actors that communicate by asynchronous message
passing, with Erlang/OTP-inspired building blocks: process links and monitors,
`trap_exit`-style supervision, supervisors, a `gen_server`-like server behaviour,
timers, a name service, and optional multi-node messaging.

> Status: pre-1.0 (`0.7.x`). The API still moves between minor versions. See
> [`CHANGELOG.md`](CHANGELOG.md).

## Concepts

- **Actor** — an `async` function that receives an exclusive reference to a
  *context* and interacts with the system only through traits on that context
  (send/receive, spawn, link, watch, bind, …). The concrete context type stays
  opaque to the actor.
- **Message** — any type marked with `#[message]`. Addresses are opaque,
  IPv6-like values; a subnet is an address plus a net-mask.
- **Supervision** — link/monitor signals plus *uniform* (many children of one
  kind) and *mixed* (a fixed set of heterogeneous children) supervisors.

## Example: defining and matching messages

```rust
use std::time::Duration;

use mm1::address::Address;
use mm1::proto::message;

// `#[message]` derives serde and marks the type as sendable between actors.
// The crate that uses it must depend on `serde` directly.
#[message]
struct Ping {
    reply_to: Address,
    seq:      u64,
}

#[message]
struct Pong {
    seq: u64,
}
```

Inside an actor you receive an `Envelope` and match on it with `dispatch!`:

```rust,ignore
use mm1::core::envelope::dispatch;

loop {
    let envelope = ctx.recv().await?;
    dispatch!(match envelope {
        Ping { reply_to, seq } => {
            ctx.tell(reply_to, Pong { seq }).await.ok();
        }
        // `dispatch!` requires a catch-all today, otherwise an unexpected
        // message panics the actor.
        other @ _ => {
            // handle or ignore
        }
    });
}
```

For complete, runnable programs — starting a node, wiring config, supervision,
and multi-node messaging — see the crates under
[`tests/`](tests/) (for example `tests/mm1-node-tests` and the
`tests/mm1-multinode-tests/examples` directory).

## Features

The umbrella `mm1` crate re-exports the subsystem crates behind feature flags.

Default: `ask`, `runtime`, `server`, `sup`, `timer`, `multinode`.

| feature        | what it enables                                   | default |
| -------------- | ------------------------------------------------- | :-----: |
| `runtime`      | the node runtime (`Rt`, config)                   |   yes   |
| `ask`          | request/response (the "ask" pattern)              |   yes   |
| `server`       | the `gen_server`-like server behaviour            |   yes   |
| `sup`          | supervisors (uniform + mixed)                     |   yes   |
| `timer`        | timers                                            |   yes   |
| `multinode`    | multi-node messaging (implies `runtime`)          |   yes   |
| `name-service` | register/resolve actors by name                   |   no    |
| `logger`       | logging setup helpers (`mm1::logger`)             |   no    |
| `test-util`    | test runtime (`mm1::test`)                        |   no    |

## Minimum supported Rust version

`mm1` builds on Rust **1.91** and later. Bumping the MSRV is treated as a
routine change, not a breaking one, while the crate is pre-1.0.

## License

MIT. See [`LICENSE`](LICENSE).
