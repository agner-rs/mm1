---
name: mm1
description: Use this skill when writing or reviewing Rust code that uses the `mm1` actor framework (crate `mm1`, repo `agner-rs/mm1`, docs at https://docs.rs/mm1) — implementing actors, supervisors, message types, ask patterns, or the runtime bootstrap.
---

# Working with mm1

`mm1` is an Erlang/OTP-inspired actor framework for Rust. Mental model: **actors are async functions that hold a context handle, communicate by typed messages, and are managed by supervision trees.** If you know OTP, the mapping is direct: `gen_server` → `mm1::server`, `supervisor` → `mm1::sup`, `gen_statem` → DIY with `recv`/`dispatch!`, `proc_lib:start` → `Rt::create().run(...)`.

For **multi-node clusters** (cross-process or cross-host messaging via TCP / UDS, protocol registration, subnet allocation, routing), see the separate **`mm1-multinode`** skill.

## Crate layout

| Module | Purpose |
|---|---|
| `mm1::runtime` | Bootstrap: `Rt::create(config).run(runnable)` |
| `mm1::core::context` | Capability traits (`Messaging`, `Fork`, `Start`, `Stop`, `Linking`, `Watching`, `InitDone`, `Quit`, `Bind`, `Now`, `Tell`) |
| `mm1::core::envelope` | `Envelope` + `dispatch!` macro for type-erased message handling |
| `mm1::proto` | `#[message]` attribute + `Message` trait |
| `mm1::server` | `OnMessage` / `OnRequest` traits for long-lived handler actors |
| `mm1::sup` | `MixedSup` (named children), `UniformSup` (dynamic same-type children), strategies, restart intensity |
| `mm1::ask` | `ctx.ask(addr, req, timeout)` — request/response with correlation |
| `mm1::address` | `Address`, `NetAddress`, `NetMask`, `AddressPool` (distributed-ready) |
| `mm1::timer` | Scheduled message delivery |
| `mm1::multinode` | Distributed actor support |
| `mm1::runnable` (re-export of `mm1_runnable`) | `local::boxed_from_fn(...)` to wrap an async fn as a runnable |

Latest version: 0.7.21. Docs coverage is partial — when an API is unclear, read the tests under `tests/mm1-*-tests/` in the repo; they are the de-facto usage examples.

**Depend on the `mm1` facade, not on the sub-crates.** The sub-crates (`mm1-core`, `mm1-proto`, `mm1-address`, `mm1-server`, `mm1-node`, `mm1-multinode`, `mm1-logger`, ...) are internal to mm1's organization. Downstream code should import from `mm1::…`. The `#[message]` macro also assumes this — its default `base_path` is `::mm1::proto`, so consumers who try to depend only on `mm1-proto` get unresolved-path errors.

## Facade path cheatsheet

Re-export paths that are easy to mis-remember (they don't follow the sub-crate's internal module structure 1:1):

| What you want | mm1 facade path |
|---|---|
| `Never` (uninhabited) | `mm1::common::Never` (not `mm1::common::types::Never`) |
| `AnyError` (= `eyre::Report`) | `mm1::common::error::AnyError` (not `…::types::AnyError`) |
| `NetAddress` | `mm1::address::NetAddress` (not `mm1::address::subnet::NetAddress`) |
| `OnMessage`, `OnRequest`, `Outcome` | `mm1::server::behaviour::{OnMessage, OnRequest, Outcome}` (under `behaviour`, not at `server::` root) |
| `Address`, `NetMask`, `AddressPool` | `mm1::address::{Address, NetMask, AddressPool}` |
| `Messaging`, `Tell`, `Fork`, `Bind`, … | `mm1::core::context::{…}` |
| `dispatch!` macro | `mm1::core::envelope::dispatch` |
| `BoxedRunnable`, `local::boxed_from_fn` | `mm1::runnable::local::{BoxedRunnable, boxed_from_fn}` |
| `Rt` (runtime), `Local` (= `BoxedRunnable<ActorContext>`) | `mm1::runtime::{Rt, Local}` |
| `MULTINODE_MANAGER`, `Protocol` | `mm1::multinode::{MULTINODE_MANAGER, Protocol}` |
| `RegisterProtocolRequest/Response` | `mm1::multinode::proto::{…}` |

Note: `mm1::common::error::AnyError` is a *type alias* for `eyre::Report`, not `Box<dyn std::error::Error + Send + Sync>`. `mm1_logger::AnyError` is a different `Box<dyn …>` type — convert at the boundary with `mm1_logger::init(&cfg).map_err(|e| eyre::eyre!("{e}"))?`.

## Bootstrap pattern

```rust
use mm1::common::error::AnyError;
use mm1::runnable::local;
use mm1::runtime::Rt;

fn main() -> Result<(), AnyError> {
    let _ = mm1_logger::init(&mm1_logger::LoggingConfig {
        min_log_level: mm1_logger::Level::INFO,
        log_target_filter: vec!["*=INFO".parse()?],
    });

    Rt::create(Default::default())?
        .run(local::boxed_from_fn(main_actor))?;
    Ok(())
}

async fn main_actor<C>(ctx: &mut C) -> Result<(), AnyError>
where
    C: mm1::core::context::Messaging + /* + capability bounds */,
{
    // ...
    Ok(())
}
```

Note: `local::boxed_from_fn` for an arg-bearing actor takes a **tuple of (function, tuple-of-args)** — the args are themselves a tuple, not flat alongside `F`:

```rust
// zero args: pass the fn directly
local::boxed_from_fn(main_actor)

// one arg: (fn, (one,))   ← note the trailing comma, it's a 1-tuple
local::boxed_from_fn((main_actor, (args,)))

// two args: (fn, (a, b))
local::boxed_from_fn((worker, (reply_to, delay)))
```

Getting this wrong (e.g. `(main_actor, args)` flat) produces an opaque `ActorRunBoxed` trait-bound error — the compiler suggestion is unhelpful.

The actor's return type can be `Result<(), E>`, `Result<Never, E>`, `()`, or `Never` (all impl `ActorExit`); see "Long-lived actors that never exit" below.

**Gotcha:** `Result<(), E>` and `Result<Never, E>` (and bare `()`) all require `Ctx: Quit` to satisfy `ActorExit<Ctx>` — only `Never` itself doesn't. When this bound is missing, the compiler surfaces it as an unhelpful "the trait `ActorFuncInner` is not implemented for `(Func, (T0,))`" error against `boxed_from_fn`, listing the multi-arg impls but hiding the actual `Quit` requirement. If you see that error and the args look right, the first thing to check is whether your actor's `C` bound includes `Quit`.

For **multinode** setup (configuring transports, registering protocols, allocating subnets across nodes), see the separate **`mm1-multinode`** skill. The short version: `Rt::create(Mm1NodeConfig { local_subnets, inbound, outbound, ... })?.run(...)`, with protocol codecs registered at *runtime* via `Protocol::new().with_type::<T>()` sent to `MULTINODE_MANAGER` — not at `Rt::create` time.

## Actor signature — capability traits, à la carte

Actors are async functions whose first parameter is `&mut C`, where `C` is a generic context constrained by exactly the capabilities the actor uses:

```rust
async fn worker<C>(ctx: &mut C, reply_to: Address, delay: Duration) -> Never
where
    C: Messaging + Quit + InitDone,
{
    ctx.init_done(ctx.address()).await;
    tokio::time::sleep(delay).await;
    ctx.tell(reply_to, Hi { /* ... */ }).await.expect("tell");
    std::future::pending().await
}
```

| Trait | Provides |
|---|---|
| `Messaging` | `recv()`, `tell()`, `address()`, `send()` |
| `Tell` | Convenience trait: `tell(addr, msg).await` |
| `Ask` (`mm1::ask::Ask`) | `ask(addr, req, timeout).await` — request/response |
| `Start<Local>` | `start(runnable, link, timeout).await` → child address |
| `Stop` | `stop(addr, timeout).await` |
| `Linking` | `link(addr)` / `unlink(addr)` — bidirectional failure propagation |
| `Watching` | `watch(addr)` / `unwatch(addr)` — one-way termination notification |
| `Fork` | `fork().await?.run(closure)` — spawn a sub-context sharing the actor's identity |
| `Bind<NetAddress>` | `bind(BindArgs { bind_to, inbox_size }).await` — set the inbox/address |
| `InitDone` | `init_done(addr).await` — signal "I'm up" to supervisor |
| `Quit` | `quit(reason).await` — terminate self |
| `Now` | `Instant` provider (test-friendly clock) |

**Rule of thumb:** declare the *minimum* set of bounds. Tests can then inject mock contexts that satisfy only what's required.

## Default actor pattern: the `mm1::server` builder (gen_server analog)

**For most actors, use the `mm1::server` builder.** It is mm1's gen_server: a state struct with one `OnMessage<Ctx, M>` impl per inbound message type and one `OnRequest<Ctx, Rq>` impl per inbound request type, wired together with a fluent builder. The builder owns the `recv` loop, dispatches by `TypeId`, and integrates with `Ask`/`Reply` automatically.

```rust
use mm1::server::{self, OnMessage, OnRequest, Outcome};
use mm1::ask::proto::RequestHeader;
use mm1::core::context::Messaging;
use futures::never::Never;

struct MyState { /* ... */ }

impl<Ctx: Messaging + Send> OnMessage<Ctx, Tick> for MyState {
    async fn on_message(&mut self, _ctx: &mut Ctx, _: Tick)
        -> Result<Outcome<Tick, Never>, AnyError>
    {
        // update self, maybe schedule next tick
        Ok(Outcome::no_reply())  // or .then_stop() to exit
    }
}

impl<Ctx: Messaging + Send> OnRequest<Ctx, GetStatus> for MyState {
    type Rs = StatusReply;
    async fn on_request(&mut self, _ctx: &mut Ctx,
                        _reply_to: RequestHeader, _: GetStatus)
        -> Result<Outcome<GetStatus, Self::Rs>, AnyError>
    {
        Ok(Outcome::reply(StatusReply { /* ... */ }))
    }
}

// In the actor entry function:
let final_state = server::new::<Ctx>()
    .behaviour(MyState::new())
    .msg::<Tick>()
    .req::<GetStatus>()
    .run(ctx)
    .await?;
```

**When to use a custom `recv` loop with `dispatch!` instead:**
- State-machine actors where the *set of handled message types depends on the current state* (gen_statem-style — multiple distinct phases, each handling a different subset of messages).
- Trivially small actors where wiring the builder is more ceremony than the actor warrants.
- Cases where you need to call `ctx.recv()` with extra logic (`select!` between recv and another future, e.g. the `uds_acceptor` selects between `accept()` and `recv()`).

mm1's own source code uses the builder where it fits. Concrete reference: `components/mm1-multinode/src/actors/uds_connector.rs` wraps a `UdsConnector { ... }` state struct as the builder's behaviour, registers `OnMessage<Ctx, Connect>` and `OnMessage<Ctx, sys::Down>`, and lets the builder run the loop. The `uds_acceptor.rs` in the same directory uses a custom recv loop because it needs `select!` between socket accepts and inbound envelopes — a good example of when *not* to use the builder.

The builder dispatches by `TypeId`. **Unregistered message types are logged as `warn!("unexpected", ...)` and dropped** — wire every type you expect to receive.

### Don't `ctx.recv` mid-handler. Use `ctx.ask` if you need a reply.

Inside an `OnMessage` or `OnRequest` handler, calling `ctx.recv().await` reads directly from the actor's mailbox — bypassing the server's dispatcher. Envelopes meant for other registered handlers will be silently consumed (the dispatcher never sees them).

The safe way to "wait for a reply" mid-handler is `ctx.ask(addr, req, timeout).await` (the trait `mm1::ask::Ask`). The default `ask` requires `Self: Fork`; internally it forks the context so the reply lands on the **fork's** mailbox, not the parent's. The parent's queue is untouched. The two ask variants are deliberate:

| Variant | Mailbox used for the reply | When to use |
|---|---|---|
| `ctx.ask` (requires `Fork`) | A forked mailbox (separate from the parent) | Default. Safe to call from inside `OnMessage`/`OnRequest`. |
| `ctx.ask_nofork` | The parent's mailbox | When the caller is not inside a server-builder loop and can drain its own mailbox; or for hot paths where the fork allocation is measured to hurt. Will conflict with `mm1::server` dispatch if used inside a handler. |

Note: `ctx.ask` only works against a **responder that uses `OnRequest`** (or that manually emits the `mm1::ask::Response` wrapper). An actor that uses `OnMessage` + `ctx.tell(reply_to, …)` (the gen_server style) does NOT satisfy `ask`'s correlation contract — `ask` would time out waiting for a wrapped Response that never arrives. Plan the protocol's reply shape accordingly.

## Messages

Define with `#[message]` (attribute macro from `mm1::proto`):

```rust
use mm1::proto::message;

#[derive(Debug)]
#[message]
struct Hi {
    worker_id: usize,
    worker_address: Address,
}
```

**`#[message]` already derives `Serialize` + `Deserialize`** and emits `impl Message for Self`. **Do NOT add your own `#[derive(Serialize, Deserialize)]`** — you'll get a "conflicting implementations" error. If you ever need to opt out (e.g. when wrapping a type that can't be serialized via the default), the macro accepts `#[message(derive_serialize = false, derive_deserialize = false)]`.

The macro emits trait paths against `::mm1::proto`. That's why depending on the `mm1` facade (not `mm1-proto` directly) is the documented path — see "Crate layout" above.

Receive and dispatch (the **escape hatch** form; prefer the `mm1::server` builder for most actors — see above):

```rust
use mm1::core::envelope::dispatch;

loop {
    let envelope = ctx.recv().await?;
    dispatch!(match envelope {
        hi @ Hi { .. } => { /* handle */ }
        Stop {} => break,
    });
}
```

`recv()` returns an `Envelope` (type-erased). Use `dispatch!` to pattern-match by type, or `envelope.cast::<T>()` for an explicit cast. This pattern shines for state-machine actors (where the handled set varies per state) and `select!`-style actors (where you wait on `recv` together with another future).

### `dispatch!` is full pattern-matching by message type

It supports guards, bindings, wildcards, and generic types — confirmed by `tests/mm1-core-tests/tests/dispatch_playground.rs`:

```rust
dispatch!(match envelope {
    ping @ Ping { seq_num, .. } if *seq_num < 10  => { /* small */ }
    ping @ Ping { seq_num, .. } if *seq_num > 10  => { /* large */ }
    ping @ Ping { .. }                            => { /* exactly 10 */ }

    AUnit                                         => { /* unit struct */ }
    ATuple(l, r) if l < r                         => { /* tuple struct, guarded */ }
    ATuple { .. }                                 => { /* tuple wildcard */ }
    AStruct { s, .. } if s == "1"                 => { /* fielded guard */ }

    Forward::<Ping> { forward_to, .. } if *forward_to == ADDR => { /* generic */ }
    Forward::<Ping> { .. } => { /* generic Ping */ }
    Forward::<Pong> { .. } => { /* generic Pong */ }
})
```

Unmatched types **panic** — if you can't enumerate all types you might receive, end with a wildcard arm or use `envelope.cast::<T>()`.

## Supervisors

### MixedSup — named children of different types (the "org chart")

Use for fixed structural roles where each child plays a distinct, named part in the supervision tree.

```rust
use mm1::sup::common::{ActorFactoryMut, ChildSpec, InitType, RestartIntensity};
use mm1::sup::mixed::{self, strategy::OneForOne, ChildType, MixedSup};

let sup = MixedSup::new(OneForOne::new(RestartIntensity {
    max_restarts: 3,
    within: Duration::from_secs(30),
}))
.with_child(
    "primary".to_string(),
    ChildSpec::new(ActorFactoryMut::new(|()| local::boxed_from_fn(primary_actor)))
        .with_child_type(ChildType::Permanent)
        .with_init_type(InitType::WithAck { start_timeout: Duration::from_secs(1) })
        .with_stop_timeout(Duration::from_secs(3)),
)
.with_child("secondary".to_string(), /* ... */);

mixed::mixed_sup(ctx, sup).await?;
```

### UniformSup — dynamic same-type children (the "ephemeral worker pool")

Use when a manager spawns identical workers on demand.

```rust
use mm1::sup::common::{ActorFactoryMut, ChildSpec, InitType};
use mm1::sup::uniform::{child_type, uniform_sup, UniformSup};
use mm1::sup::proto::uniform;

let factory = ActorFactoryMut::new(|args: WorkerArgs| {
    local::boxed_from_fn((worker, args))
});
let spec = ChildSpec::new(factory)
    .with_child_type(child_type::Temporary)
    .with_init_type(InitType::WithAck { start_timeout: Duration::from_secs(1) })
    .with_stop_timeout(Duration::from_secs(1));

let sup = UniformSup::new(spec);
let sup_addr = ctx
    .start(local::boxed_from_fn((uniform_sup, (sup,))), true, Duration::from_secs(1))
    .await?;

// Spawn a child on demand
let started: uniform::StartResponse = ctx
    .ask(sup_addr, uniform::StartRequest { args: worker_args }, Duration::from_millis(100))
    .await?;
let child_addr = started?;

// Terminate it later
let _: uniform::StopResponse = ctx
    .ask(sup_addr, uniform::StopRequest { child: child_addr }, Duration::from_millis(100))
    .await?;
```

### Child types (OTP semantics)

| Type | Restart on exit? |
|---|---|
| `Permanent` | Always restart |
| `Temporary` | Never restart |
| `Transient` | Restart only on abnormal exit |

### Restart strategies

- `OneForOne` — restart only the failed child (most common).
- Other strategies (`OneForAll`, `RestForOne`) likely exist in `mm1::sup::mixed::strategy`; confirm in code before assuming.

## Messages have no source address — replies go in the payload

mm1 envelopes carry a destination header but **no source address**. To receive a reply, embed `reply_to: Address` in the request payload — the gen_server / OTP pattern. This is also true cross-node.

```rust
#[derive(Debug)] #[message]
struct PingRequest { reply_to: Address, seq: u64 }

#[derive(Debug)] #[message]
struct PingReply { seq: u64 }

// requester
ctx.tell(target, PingRequest { reply_to: ctx.address(), seq: 1 }).await?;
let envelope = ctx.recv().await?;
let (PingReply { seq }, _) = envelope.cast::<PingReply>().unwrap().take();

// responder
let env = ctx.recv().await?;
let (PingRequest { reply_to, seq }, _) = env.cast::<PingRequest>().unwrap().take();
ctx.tell(reply_to, PingReply { seq }).await?;
```

The `ask` machinery below wraps this pattern automatically. For application-level authentication, put credentials in the payload too — there is no message-level "from" the receiver can trust. Treat received `reply_to` fields as untrusted input until verified.

## Ask pattern (request/response)

### Client side

```rust
use mm1::ask::{Ask, AskErrorKind};

let response: MyResponse = ctx
    .ask(target_addr, MyRequest { /* ... */ }, Duration::from_millis(500))
    .await?;

// `ask_nofork` — does not fork the context for the reply mailbox; lower overhead
// when you can guarantee the reply will arrive on the current context.
let response: MyResponse = ctx
    .ask_nofork::<MyRequest, MyResponse>(addr, req, timeout)
    .await?;
```

`AskError::kind()` gives `AskErrorKind::Timeout`, etc. — match on it for timeout-specific behavior.

### Server side

**Use the `mm1::server` builder** — covered above under [Default actor pattern](#default-actor-pattern-the-mm1server-builder-gen_server-analog). Implement `OnRequest<Ctx, Rq>` on your state struct (with `type Rs = ...`), register it with `.req::<Rq>()`, and the builder handles `Request<Rq>` decode, correlation, and `Response<Rs>` reply.

**Lightweight escape hatch:** if you have a single one-off `ask` handler and don't want to wire the builder, implement the actor manually with the `Reply` capability:

```rust
use mm1::ask::{Reply, proto::Request};
use mm1::core::envelope::dispatch;

async fn server<Ctx: Reply>(ctx: &mut Ctx) {
    while let Ok(envelope) = ctx.recv().await {
        let (header, _payload) = dispatch!(match envelope {
            Request::<Rq> { header, payload } => (header, payload),
        });
        ctx.reply(header, Rs).await.unwrap();
    }
}
```

The `header: RequestHeader` carries the correlation id and the requester's `reply_to` address — pass it to `ctx.reply` and the framework routes the `Response<Rs>` back automatically. Prefer the builder for anything beyond a single request type.

## Spawning vs forking

| Operation | Identity | Use for |
|---|---|---|
| `ctx.start(runnable, link, timeout)` | **new actor**, new address | A real child actor with its own lifecycle |
| `ctx.fork().await?.run(closure).await` | **same actor**, different context handle | A concurrent sub-task within one actor (e.g., a sender loop alongside the receive loop) |

Forks share the actor's identity for supervision purposes; spawned children are independent supervised entities.

## Timers

`mm1::timer::v1::OneshotTimer` (from `tests/mm1-timer-tests/tests/via-mm1-node.rs`):

```rust
use mm1::timer::v1::OneshotTimer;

let mut timers = OneshotTimer::create(ctx).await?;
timers.schedule_once_after(after, payload).await?;
// `payload` arrives as a normal message after `after` elapses
```

The scheduled `payload` is delivered as a regular envelope to the actor's mailbox — receive and dispatch as usual.

## Testing

`mm1::test::rt::TestRuntime` drives an actor event-by-event and lets you assert on each effect:

```rust
use mm1::test::rt::{TestRuntime, query, MainActorOutcome};
use tokio::time;

#[tokio::test]
async fn it_works() {
    time::pause();  // deterministic clock

    let rt = TestRuntime::<()>::new();
    let subnet = AddressPool::new("<ff:>/16".parse().unwrap());
    let lease = subnet.lease(NetMask::M_32).unwrap();
    let addr = lease.address;

    rt.add_actor(addr, Some(lease), my_actor).await.unwrap();

    // Expect a Recv, resolve it with an envelope you construct:
    let recv = rt.next_event().await.unwrap().unwrap()
        .convert::<query::Recv>().unwrap();
    recv.resolve_ok(some_envelope);

    // Expect a Tell, capture/assert the outgoing envelope:
    let mut tell = rt.next_event().await.unwrap().unwrap()
        .convert::<query::Tell>().unwrap();
    let envelope = tell.take_envelope();
    assert_eq!(envelope.header().to, expected_target);
    tell.resolve_ok(());
}
```

Available event types: `query::Recv`, `query::Tell`, `query::Spawn`, `query::Fork`, `query::ForkRun`, `query::Link`, `query::SetTrapExit`, `query::InitDone`, plus `MainActorOutcome` for actor exit. `uni_sup_basic.rs::test_02` is the most thorough example.

## Long-lived actors that never exit

For an actor that should never voluntarily return (a service supervisor's child, a long-running worker), idiomatic shape is `eyre::Result<Never>`:

```rust
async fn run<C>(ctx: &mut C, args: Args) -> eyre::Result<Never>
where C: Messaging + Quit + InitDone + Send,
{
    // …setup…
    ctx.init_done(ctx.address()).await;
    // …server loop / select / etc…
    let never: Never = std::future::pending::<Never>().await;
    match never {}  // explicitly drains the uninhabited type
}
```

`ActorExit` is implemented for both `Result<(), E>` and `Result<Never, E>` (`mm1-core/src/actor_exit.rs`), so either compiles, but `Result<Never, E>` makes the "this function never returns Ok" guarantee explicit and removes the dummy `Ok(())` line at the end.

## Gotchas

- **Doc coverage is partial (~38%).** When an API surface is unclear, read `tests/mm1-*-tests/` in the repo; tests are authoritative.
- **`#[message]` is required**, not optional. A plain Rust struct cannot be sent.
- **`#[message]` auto-derives `Serialize` + `Deserialize`.** Don't add your own derives on top (conflicting impls). Override with `#[message(derive_serialize = false, derive_deserialize = false)]` when wrapping a non-serde type.
- **Capability bounds are intentional.** Don't widen them to `Messaging + Fork + Start + Stop + ...` everywhere — the minimal bound *is* the actor's API contract.
- **`InitType::WithAck` matters.** A supervisor with `WithAck` will block until the child calls `ctx.init_done(addr).await`. Forgetting this hangs startup.
- **`Never` return type** is idiomatic for actors that should never voluntarily exit (workers that loop forever). Use it where applicable; it lets the supervisor own the lifecycle.
- **`AddressPool` + `NetMask`** are for distributed addressing. For local-only work, `Default::default()` runtime config is fine.
- **`mm1_logger` is a demo logger**, not production-grade. It's a thin `tracing-subscriber` wrapper with an exact-match path-segment filter trie (no wildcards, no parent-falls-through, no `RUST_LOG` integration) and a hard-coded `.pretty()` formatter. Real binaries should configure `tracing-subscriber` directly with `EnvFilter`.
- **`init_done` MUST take the actor's primary address (`ctx.address()`), not a bound discovery address.** `ctx.start(runnable, link=true, …)` links the parent to whatever address the child reports via `init_done`. If you pass a bound secondary address (set up via `ctx.bind(BindArgs { bind_to: STATIC_ADDR, … })`), the parent's link targets that secondary; the child's primary container rejects the Link Connect with `Link(Exit { reason: LinkDown })`; the parent dies — and because of the upstream quirk that `Rt::run` swallows actor errors, the process exits 0 with no panic, no error log. Always `ctx.init_done(ctx.address())`. Bound static addresses still serve their service-discovery purpose; only the link target moves to primary.

## References

- Crate docs: https://docs.rs/mm1
- Source: https://github.com/agner-rs/mm1

Authoritative usage examples in the repo (read these first when an API is unclear):

| File | Shows |
|---|---|
| `tests/mm1-sup-tests/tests/uni_sup_basic.rs` | `UniformSup` + `StartRequest`/`StopRequest`, `TestRuntime` walk-through |
| `tests/mm1-sup-tests/tests/mixed_sup_basic.rs` | `MixedSup` with named children, `OneForOne`, `ChildType::Permanent`, `InitType::WithAck` |
| `tests/mm1-ask-tests/tests/ergonomics.rs` | `Ask` + `Reply` traits, `ask_nofork`, server dispatch on `Request::<T>` |
| `tests/mm1-core-tests/tests/dispatch_playground.rs` | Full power of `dispatch!` — guards, generics, wildcards |
| `tests/mm1-timer-tests/tests/via-mm1-node.rs` | `OneshotTimer` usage |
| `tests/mm1-tests/examples/a-node.rs.wip` | Multinode bind/peer with `Codec::with_type::<T>()` |
| `components/mm1-server/src/lib.rs` | `Server` builder source — `behaviour`, `msg::<M>`, `req::<Rq>`, `run` |
| `components/mm1-server/src/behaviour.rs` | `OnMessage` / `OnRequest` trait definitions |
