// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use proc_macro::TokenStream;

mod dispatch;
mod message;

#[proc_macro_attribute]
pub fn message(attr: TokenStream, item: TokenStream) -> TokenStream {
    message::message(attr, item)
}

/// Dispatch an `Envelope` over a `match`-like set of typed arms.
///
/// `dispatch!(match envelope { Foo(f) => .., Bar { .. } => .. })` tests the
/// envelope's message against each arm's message type in order and runs the
/// body of the first that matches.
///
/// # Supported patterns
///
/// Typed arms support unit-struct paths, tuple-struct patterns, struct
/// patterns, and bindings such as `message @ Foo { .. }`. Guards are supported
/// on typed arms, but they inspect the message before the envelope is consumed.
/// Their bindings are therefore borrowed: for example, write
/// `Ping { seq } if *seq > 10` when `seq` is a `u64`.
///
/// Use `_` to ignore every remaining message, or `message @ _` to bind the
/// catch-all message. Catch-all arms cannot have guards; put the condition in
/// the arm body instead. A bare lowercase identifier is rejected as ambiguous
/// with the unit-struct pattern syntax, so binding catch-alls must use
/// `message @ _`.
///
/// # Unmatched messages
///
/// If no arm matches and there is no catch-all arm (e.g. `_ => ..` or
/// `other @ _ => ..`),
/// the message is **logged at `WARN` and dropped** (via
/// `Envelope::log_unhandled`); the actor is not crashed. This mirrors the
/// fire-and-forget semantics elsewhere in the runtime — any peer that learns an
/// address can send an unexpected message, and that must not be able to kill
/// the actor.
///
/// Because the generated fallback evaluates to `()`, a `dispatch!` used in
/// value position (its result assigned or returned) must supply its own
/// catch-all arm to produce that value.
#[proc_macro]
pub fn dispatch(input: TokenStream) -> TokenStream {
    dispatch::dispatch(input)
}
