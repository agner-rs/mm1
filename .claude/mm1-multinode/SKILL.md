---
name: mm1-multinode
description: Use this skill when working with mm1's multinode features — running an mm1 cluster, designing the wire protocol between nodes, configuring transports (TCP or UDS), registering message codecs, planning subnet/address allocation, or building services that span more than one mm1 node.
---

# mm1 multinode

The `mm1::multinode` machinery (crate `mm1-multinode`, plus `mm1-node` for runtime/config) connects independent mm1 nodes into a cluster that exchanges typed messages. Use the base `mm1` skill first for actor mechanics; this skill is exclusively about the cross-node layer.

## Mental model

- A **node** is an mm1 runtime (`Rt`) configured with:
  - one or more **local subnets** (the address space it owns),
  - zero or more **inbound** listeners (places peers can connect to),
  - zero or more **outbound** connections (peers it initiates to).
- Nodes exchange **typed messages** within named **protocols**. A protocol is a bundle of codecs (one per `#[message]` type).
- When nodes connect, they **advertise their local subnets** to each other; mm1 builds a route table from those advertisements.
- An outbound message is routed by destination `Address` matching against the route table.
- **Intermediate hops do NOT decode payloads** — they forward the opaque `Forward { local_type_key, body }`. Only the destination node looks up the type from the local protocol registry and decodes.
- The wire format for payloads is **MessagePack** (`rmp_serde`).
- There is **no built-in service discovery**. After nodes are physically connected, actors still need to know exact destination addresses to initiate communication.
- There is **no concept of source address on a message**. For replies, embed `reply_to: Address` in the payload (the gen_server pattern).

## Transports

| Scheme | Use | Notes |
|---|---|---|
| `tcp://host:port` | Cross-host links | Plaintext on the wire. |
| `uds:///path/to/sock` | Same-host links (e.g., container ↔ host) | Three slashes (empty host). Acceptor uses a `.lock` file to detect and clean up stale sockets; removes the socket on drop. Connector auto-reconnects every 1s on failure. |

Source: `components/mm1-multinode/src/actors/{tcp,uds}_{acceptor,connector}.rs`, generic stream handling in `iostream_connection.rs`.

## Authentication

Configuration value for `authc`:
- `"trusted"` — no cookie expected; both sides must agree.
- `{"cookie": "shared-secret-string"}` — cookie must match on both ends.

**The cookie is cross-wiring protection, not security.** Same role as Erlang cookies: it prevents your dev cluster from accidentally talking to your staging cluster. Traffic is plaintext, no encryption, no integrity beyond the transport. If a connection is between processes that might be untrusted, do not rely on the cookie:

- For TCP across hosts: TLS-wrap the transport at the OS/process level (e.g., spiped, stunnel, WireGuard), or run mm1 over an authenticated tunnel.
- For UDS: rely on filesystem permissions and mount visibility (only the containers/processes that should connect have the socket path).
- For application-level authentication of peer actors: put credentials **in the message payload**. mm1's wire layer has no concept of message authentication.

## Configuration shape

`Mm1NodeConfig` (in `mm1-node/src/config.rs`) is serde-deserializable from JSON/YAML/TOML:

```json
{
  "local_subnets": [
    { "net": "<aa0:>/16", "kind": "auto" },
    { "net": "<aa1:>/16", "kind": "bind" }
  ],
  "inbound": [
    { "proto": ["my-proto"], "addr": "uds:///tmp/node.sock", "authc": "trusted" }
  ],
  "outbound": [
    { "proto": ["my-proto"], "addr": "tcp://10.0.0.1:9000", "authc": {"cookie": "abc"} }
  ]
}
```

### `local_subnets`

```rust
enum LocalSubnetKind { Auto, Bind }
```

- `auto`: **exactly one required per node**. The runtime panics on boot with "exactly one local subnet must be present" if missing. This is the subnet used for auto-allocated addresses (spawned actors that don't request a specific address).
- `bind`: zero or more. Additional reserved subnets.

Both are registered with the local multinode manager at boot via `RegisterLocalSubnetRequest { net }`. The functional distinction between `Auto` and `Bind` beyond "one required vs many allowed" is not fully clear from the code surface inspected (`mm1-node/src/init.rs` treats them the same in the registration loop). If the distinction matters for your design, **read `components/mm1-multinode/src/actors/multinode_manager.rs` or ask the mm1 author — do not guess.**

**Cluster invariant:** local subnets MUST NOT overlap across nodes. The route registry's `set_route` returns `SetRouteError::Conflict` when two nodes claim overlapping nets, and routing breaks for the conflicting region. Plan subnet allocation centrally — typically by giving one node responsibility for leasing slices of a parent subnet to other nodes as they join.

### `inbound` / `outbound`

```rust
struct DefMultinodeInbound  { proto: Vec<ProtocolName>, addr: DefAddr, ..options }
struct DefMultinodeOutbound { proto: Vec<ProtocolName>, addr: DefAddr, ..options }

enum DefAddr { Tcp(SocketAddr), Uds(Box<Path>) }
```

- `proto: [...]` — the names of protocols this interface speaks. **These protocols MUST be registered before the listener/connector becomes functional.** The acceptor calls `wait_for_protocol` with a 10s timeout; the connector with 60s.
- `addr` — TCP or UDS URL.
- `authc` — see above; flattened into the `options` field.

## Protocols — runtime registration

Protocols are built and registered **at runtime by the main actor** (or any actor with messaging context), not at `Rt::create` time:

```rust
use std::time::Duration;
use mm1::multinode::{MULTINODE_MANAGER, Protocol};
use mm1::multinode::proto::{RegisterProtocolRequest, RegisterProtocolResponse};

let protocol = Protocol::new()
    .with_type::<MyMessage1>()
    .with_type::<MyMessage2>();

let _: RegisterProtocolResponse = ctx
    .ask(
        MULTINODE_MANAGER,
        RegisterProtocolRequest { name: "my-proto".into(), protocol },
        Duration::from_millis(100),
    )
    .await??;
```

- `MULTINODE_MANAGER` is a well-known local address (`mm1_proto_well_known::MULTINODE_MANAGER`, re-exported as `mm1::multinode::MULTINODE_MANAGER`).
- `#[message]` (see base `mm1` skill) already derives `Serialize` + `Deserialize` and emits `impl Message`. The trait bounds the multinode layer needs (`Serialize + DeserializeOwned + Send + 'static`) are satisfied by construction for any `#[message]` type whose fields are themselves serializable + `Send`. You do not need to add the derives or list the bounds manually.
- The same protocol **name** must be used on both sides for messages of types registered under it to be exchanged. The types themselves must serialize-compatibly (same fields, same MessagePack representation).
- After registration, inbound/outbound interfaces that named this protocol become functional.

### Protocol crates — own them separately

Keep `#[message]` types in protocol-only crates owned by neither client nor server. Example layout for a system where a controller talks to a server, and the server talks to per-task workers:

```
myapp-proto-controller-server/   # controller↔server messages
  src/lib.rs
  Cargo.toml          # deps: mm1 (facade), serde
myapp-proto-server-worker/       # server↔worker messages
myapp-server/                    # depends on both
myapp-worker/                    # depends on the server-worker crate
```

This avoids circular dependencies, keeps wire types versioned independently from implementations, and makes wire compatibility a deliberate change. Note: protocol crates depend on the `mm1` facade (not the sub-crates) — `#[message]` emits paths against `::mm1::proto`.

## Routing

The route registry (`RouteRegistry` in `route_registry.rs`) maps `(LocalTypeKey, dst_net) → (gateway, metric)`:

- **Per-type routes.** Routes are keyed by message type — *different message types CAN take different routes to the same destination*. Used when capabilities or load characteristics differ per type. For most cases, all types route the same.
- **Multiple candidates per destination** — up to 4. The router consistent-hashes on destination `Address` to pick one (light-touch load balancing).
- **Subnet overlap rejected.** Adding a route whose destination net overlaps an existing route for the same type returns `SetRouteError::Conflict`. This is the enforcement of the no-overlap invariant.
- **Routes are advertised between nodes** via `SubscribeToRoutesRequest` / `SubscribeToRoutesResponse` / `SetRoute` messages (`proto.rs`). New connections trigger an exchange.
- **Forwarding is opaque.** `proto::Forward { local_type_key, body }` carries the MessagePack body; intermediate nodes don't need the codec, only the destination does.

## No built-in discovery

After nodes are physically connected, **actors still need destination addresses to initiate communication**. mm1 doesn't ship a cluster-wide naming service (the optional `name-service` feature is a local-only directory).

Strategies for first-contact:
- **Well-known addresses.** `MULTINODE_MANAGER` is one. Define your own (e.g., a service-directory actor at a fixed address per node).
- **Bootstrap via argv / config.** Pass peer addresses in at startup — a launched node receives the address of its peer as a CLI argument or env var.
- **Welcome handshake.** On connect, a small handshake exchanges addresses of named services (one node sends a `Welcome { services: { … }, … }` reply to the other on first contact).

**Do NOT rely on "predict the address and you can talk to me" as a security boundary.** Actor IDs are not cryptographically unguessable — security-through-obscurity is a bad design here.

## Messages have no source address — replies go in the payload

mm1 envelopes carry a destination header but no source address. To receive a reply, embed `reply_to: Address` in the request payload (the gen_server / OTP pattern):

```rust
#[derive(Debug)] #[message]
struct PingRequest {
    reply_to: Address,
    seq: u64,
}

#[derive(Debug)] #[message]
struct PingReply { seq: u64 }

// requester
ctx.tell(target, PingRequest { reply_to: ctx.address(), seq: 1 }).await?;
let envelope = ctx.recv().await?;
let reply: PingReply = envelope.cast().unwrap().take().0;

// responder
let env = ctx.recv().await?;
let (PingRequest { reply_to, seq }, _) = env.cast().unwrap().take();
ctx.tell(reply_to, PingReply { seq }).await?;
```

The `ask` machinery (`mm1::ask::Ask`) wraps this pattern automatically: it generates a correlation id, embeds `reply_to`, and matches the response. For pure request/response, prefer `ctx.ask(addr, req, timeout)` — see the base `mm1` skill.

### Sender identity is not surfaced by mm1 — auth lives in the payload

mm1's inbound listeners decode messages from connections, then dispatch each message to its destination `Address` via the route table. The recipient actor **has no way to tell which connection a message arrived on**, which transport it came over, or which inbound listener handled it. There is no API that returns "which link did this envelope come from", because mm1's internal routing dissolves that information by design. Anyone who tells you they wrote an actor that "knows which UDS the message came from" is mistaken — they're either confused about what mm1 exposes or they're reading information out of the payload they pretended was from the wire.

(Concretely: the actor's `ctx.recv` returns an `Envelope` with a destination header and a payload. There is no source-connection field, no `originating_listener` API, no "ask the multinode manager which link delivered this envelope" — that information is not retained past the decode step.)

So any service that needs a **trusted** sender identity — e.g. a message-routing service that records who sent each message and must not let the sender forge that field — has to get that identity from the **payload**, and validate it. The cookie on the transport gives cross-wiring protection only (see "Authentication" above). The canonical pattern:

1. The system issues a credential to each authorised principal out-of-band (boot env var, handshake, etc.). The credential identifies the principal and can be checked against a credential store.
2. Every authenticated request carries `{ principal_id, credential, payload: T }` on the wire — a small generic envelope around the operation. The principal_id is the *claim*; the credential is the *proof*.
3. The recipient service validates `(principal_id, credential)` against the credential store on every request (or caches with a revocation strategy). If valid, it processes the unwrapped `T`. If not, it rejects.
4. Tests construct the wrapped envelope directly — there's no "acceptor" intermediate to mock.

You can choose to centralise validation in a gateway actor (which then `Outcome::forward`s the unwrapped payload), or do it inline in each service handler. Both work; the gateway adds a hop but de-duplicates the validation logic.

### Cross-actor lookups inside a single node — use `ctx.ask`, not shared state

If two service actors live on the same mm1 node and one needs to query the other (e.g. one service resolves a recipient identity via a directory service), use `ctx.ask(addr, req, timeout)` from inside the requesting actor's handler. `ask` requires `Self: Fork` precisely because it forks the context and routes the reply through the **fork's** mailbox — the parent's queue (which the server-builder dispatcher is draining) is untouched. The two `ask` variants are documented in the base `mm1` skill ("Don't `ctx.recv` mid-handler").

This means **the responder must use `OnRequest`**, not `OnMessage` + manual `ctx.tell(reply_to, …)`. `ask` correlates against `mm1::ask::Response`, which `OnRequest` emits via `Outcome::reply`; an `OnMessage` handler that manually tells back to `reply_to` does NOT satisfy the correlation contract and `ask` will time out.

It's tempting to bypass this with an `Arc<RwLock<State>>` shared between in-process actors — and it works (zero round-trip, synchronous read). But it splits ownership of the state across the actor and its callers, and breaks once the same service must answer cross-process callers too. Prefer `ctx.ask` unless you've measured the round-trip is hurting and the state is genuinely read-only across actors.

## Bootstrap timing — protocols THEN connections

The order of operations when starting a multinode runtime:

1. `Rt::create(config)?` parses config and starts the multinode manager.
2. Inbound listeners and outbound connectors start, but **block waiting for the protocols they named** (`wait_for_protocol` — 10s timeout for acceptors, 60s for connectors).
3. The main actor runs. It registers protocols via `RegisterProtocolRequest`.
4. Listeners/connectors unblock; connections come up; routes propagate.
5. Cross-node messages start flowing.

**This means: if you register protocols too late, the acceptor times out and the listener silently fails.** Register early in the main actor. The tests use `tokio::time::sleep(Duration::from_secs(2))` after registration to let routes propagate before any cross-node send — this is a real concern; the route advertisement is not instantaneous.

## Worked example — two nodes over UDS

Condensed from `tests/mm1-multinode-tests/tests/mulitple-nodes.rs`:

```rust
use std::time::Duration;
use mm1::ask::Ask;
use mm1::core::context::{Messaging, Tell};
use mm1::multinode::{MULTINODE_MANAGER, Protocol};
use mm1::multinode::proto::{RegisterProtocolRequest, RegisterProtocolResponse};
use mm1::proto::message;
use mm1::runnable::local;
use mm1::runtime::Rt;
use mm1_multinode::actors::context::ActorContext;
use serde_json::json;

#[message]
struct Hi;

let config_listener = json!({
    "local_subnets": [
        { "net": "<aa0:>/16", "kind": "auto" },
        { "net": "<aa1:>/16", "kind": "bind" }
    ],
    "inbound": [
        { "proto": ["proto"], "addr": "uds:///tmp/test.sock", "authc": "trusted" }
    ]
});
let config_dialer = json!({
    "local_subnets": [
        { "net": "<aa2:>/16", "kind": "auto" },
        { "net": "<aa3:>/16", "kind": "bind" }
    ],
    "outbound": [
        { "proto": ["proto"], "addr": "uds:///tmp/test.sock", "authc": "trusted" }
    ]
});

// On each node, in the main actor:
async fn main_actor<Ctx: ActorContext>(ctx: &mut Ctx) -> Result<(), AnyError> {
    let _: RegisterProtocolResponse = ctx.ask(
        MULTINODE_MANAGER,
        RegisterProtocolRequest {
            name: "proto".into(),
            protocol: Protocol::new().with_type::<Hi>(),
        },
        Duration::from_secs(1),
    ).await??;

    // settle for route propagation
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ... now ctx.tell(<peer-address>, Hi).await works across nodes
    Ok(())
}

// Bootstrap each:
let rt = Rt::create(serde_json::from_value(config_listener)?)?;
rt.run(local::boxed_from_fn(main_actor))?;
```

Note: both subnet sets (`<aa0:>/16`, `<aa1:>/16` on node 1, and `<aa2:>/16`, `<aa3:>/16` on node 2) are non-overlapping cluster-wide.

## Worked example — TCP with cookies

```rust
let bind = json!({
    "local_subnets": [{ "net": "<aa:>/16", "kind": "auto" }],
    "inbound": [
        { "proto": ["my-control-plane"],
          "addr": "tcp://0.0.0.0:9000",
          "authc": { "cookie": "shared-secret" } }
    ]
});
let dial = json!({
    "local_subnets": [{ "net": "<bb:>/16", "kind": "auto" }],
    "outbound": [
        { "proto": ["my-control-plane"],
          "addr": "tcp://10.0.0.5:9000",
          "authc": { "cookie": "shared-secret" } }
    ]
});
```

Reminder: the cookie keeps unrelated clusters apart. It does not encrypt or authenticate against a real attacker.

## `ActorContext` convenience trait

`mm1_multinode::actors::context::ActorContext` is a blanket trait that bundles every capability a multinode actor commonly needs (`Messaging + Ask + Fork + InitDone + Quit + Watching + Linking + Start + ...`). Use it as the bound for top-level multinode actors when you don't want to enumerate each trait. For library code that should be testable with narrow mocks, stick to listing the specific capabilities (see base `mm1` skill).

## Cross-node operations available via Context

| Operation | Trait | Notes |
|---|---|---|
| `ctx.tell(addr, msg).await` | `Messaging` / `Tell` | Routed by destination if `addr` is in a remote subnet |
| `ctx.ask(addr, req, timeout).await` | `mm1::ask::Ask` | Embeds reply_to + correlation id; works cross-node |
| `ctx.ping(addr, timeout).await` | `mm1::core::context::Ping` | Returns `Duration`. Useful for cross-node liveness checks (`tests/mm1-multinode-tests/tests/ping.rs`) |
| `ctx.watch(addr).await` | `Watching` | Notifies on the watched actor's termination, even cross-node (used by `uds_connector` to detect peer disconnects) |
| `ctx.link(addr).await` | `Linking` | Bidirectional failure propagation; works cross-node |

## Failure observation at the runtime level

For top-level actor failures (above your supervisors), wire an actor-failure sink before `run`:

```rust
let (tx, mut rx) = mpsc::unbounded_channel();
let rt = Rt::create(config)?.with_actor_failure_sink(tx);

// in another task:
while let Some((address, failure)) = rx.recv().await {
    error!(%address, error = %failure.as_display_chain(), "actor failure");
}

rt.run(local::boxed_from_fn(main_actor))?;
```

This is independent of supervision — useful for the topmost level of the runtime where there's no supervisor above. Pattern from `tests/mm1-multinode-tests/examples/multinode-example.rs`.

## Gotchas

- **`LocalSubnetKind::Auto` semantics.** Exactly one required per node; functional distinction from `Bind` beyond uniqueness isn't fully documented from the surfaces inspected. If subtle behavior matters, verify in `multinode_manager.rs` or ask the mm1 author.
- **Subnet overlap silently breaks routes** for the conflicting region. Plan address allocation centrally.
- **Protocols register *after* `Rt::create`** — the runtime starts listeners that wait for them. Register early; don't gate registration on something that won't happen until later.
- **Route propagation is asynchronous.** Tests sleep 2s after registration before sending. If you need ordering guarantees, wait for a `SubscribeToRoutesResponse` rather than sleeping.
- **MessagePack means breaking serde changes are wire-breaking.** Renaming a field, changing a type, removing a variant — all wire-breaking. Version your protocol crates and choose a compat strategy (rolling deploy with backward-compat releases, hard cutovers, or explicit `#[serde(rename, default)]` guards).
- **No source address; no per-connection identity surfaced to actors; no message-level authentication from the transport.** All of these have to go in the payload. There is no API that tells a recipient actor "this envelope came from this connection / this UDS / this listener" — routing dissolves that information. Treat received `reply_to` and any claimed identity fields as untrusted input until verified against an application-level credential store.
- **UDS stale socket cleanup is non-atomic.** The acceptor uses an exclusive flock on `<path>.lock`, then removes the socket if present. A second process trying to bind while the first holds the lock will fail with "socket already in use" — that's the intended behaviour, not a bug.
- **Cookie is not security.** State this in any threat model that mentions multinode.
- **Cluster-wide protocol-name agreement.** Both sides must register a protocol of the same name and serialize-compatible types. Mismatched names = no routing for those types between those nodes.
- **sqlx (and other tokio-runtime-bound resources) must be opened on mm1's runtime.** `sqlx::PgPool`'s background tasks bind to the **ambient** tokio runtime at acquire time. If you open the pool on a transient `tokio::runtime::Runtime` (e.g. one you built in `fn main()` for a pre-boot smoke check) and drop it before `Rt::create()`, the pool's tasks die with the runtime and the pool silently breaks on first real use. Open pg pools — and anything else with a similar runtime-tied lifecycle — INSIDE the main actor (which runs on mm1's runtime), not in `fn main()` before `Rt::create()`.

## References

Authoritative source code in the upstream `agner-rs/mm1` repo (commit at time of writing has version 0.7.21):

| Path | Shows |
|---|---|
| `tests/mm1-multinode-tests/examples/multinode-example.rs` | Full TCP node, CLI args, protocol registration, message exchange |
| `tests/mm1-multinode-tests/tests/mulitple-nodes.rs` | Minimal two-node UDS test |
| `tests/mm1-multinode-tests/tests/ping.rs` | Cross-node `ctx.ping` |
| `tests/mm1-multinode-tests/tests/protocol-manager.rs` | Protocol registration patterns |
| `components/mm1-multinode/src/lib.rs` | Module surface |
| `components/mm1-multinode/src/proto.rs` | `Forward`, `SetRoute`, `SubscribeToRoutes*` types |
| `components/mm1-multinode/src/codec.rs` | `Protocol`, `Known<T>`, `Opaque` codec types; rmp_serde encoding |
| `components/mm1-multinode/src/route_registry.rs` | Route storage, conflict detection, candidate selection |
| `components/mm1-multinode/src/actors/uds_acceptor.rs` | UDS bind, stale-socket cleanup |
| `components/mm1-multinode/src/actors/uds_connector.rs` | Auto-reconnect loop with `OneshotTimer` |
| `components/mm1-multinode/src/actors/multinode_manager.rs` | 38KB — the heart of multinode; consult for protocol-resolution and route-distribution internals |
| `core/mm1-node/src/config.rs` | `Mm1NodeConfig`, `LocalSubnetKind`, `DefAddr` (URL parser for `tcp://` / `uds://`) |
| `core/mm1-node/src/init.rs` | Boot sequence; local-subnet and interface registration |
