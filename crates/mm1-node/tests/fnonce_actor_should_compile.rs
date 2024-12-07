use std::future::Future;
use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_core::context::*;
use mm1_core::envelope::Envelope;
use mm1_core::types::{Never, StdError};
use mm1_node::runtime::runnable::{
    ActorRun, {self},
};
use mm1_proto::AnyError;
use tokio::time;

async fn actor_exits<C: Quit>(context: &mut C) -> Never {
    context.quit_ok().await
}

async fn actor_returns_unit<C>(_context: &mut C) {}

async fn actor_returns_result<C>(_context: &mut C) -> Result<(), std::io::Error> {
    Ok(())
}

async fn actor_returns_any_error<C>(_context: &mut C) -> Result<(), AnyError> {
    Ok(())
}

async fn actor_accepts_one_arg<C>(_context: &mut C, name: &str) {
    eprintln!("my name is {}", name);
}
async fn actor_accepts_two_args<C>(_context: &mut C, name: &str, idx: usize) {
    eprintln!("my name is {}[{}]", name, idx);
}

async fn run_it<C, A>(context: &mut C, actor: A)
where
    C: Quit + Recv,
    A: ActorRun<C>,
{
    time::timeout(Duration::from_millis(1), actor.run(context))
        .await
        .expect_err("should timeout");
    eprintln!("Exited. Address: {}", context.address());
}

struct Context(Address);

impl Recv for Context {
    fn address(&self) -> Address {
        self.0
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        Err(ErrorOf::new(RecvErrorKind::Closed, ""))
    }

    async fn close(&mut self) {}
}

impl Quit for Context {
    fn quit_ok(&mut self) -> impl Future<Output = Never> {
        std::future::pending()
    }

    fn quit_err<E>(&mut self, _reason: E) -> impl Future<Output = Never>
    where
        E: StdError + Send + Sync + 'static,
    {
        std::future::pending()
    }
}

#[tokio::test]
async fn from_fn() {
    let mut context = Context(Address::from_u64(1));

    run_it(&mut context, runnable::from_fn(actor_returns_unit)).await;
    run_it(&mut context, runnable::from_fn(actor_returns_result)).await;
    run_it(&mut context, runnable::from_fn(actor_returns_any_error)).await;
    run_it(&mut context, runnable::from_fn(actor_exits)).await;

    run_it(
        &mut context,
        runnable::from_fn((actor_accepts_one_arg, ("named actor",))),
    )
    .await;

    run_it(
        &mut Context(Address::from_u64(10)),
        runnable::from_fn((actor_accepts_two_args, ("worker", 0))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(11)),
        runnable::from_fn((actor_accepts_two_args, ("worker", 1))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(12)),
        runnable::from_fn((actor_accepts_two_args, ("worker", 2))),
    )
    .await;
}

#[tokio::test]
async fn boxed_from_fn() {
    let mut context = Context(Address::from_u64(1));

    run_it(&mut context, runnable::boxed_from_fn(actor_returns_unit)).await;
    run_it(&mut context, runnable::boxed_from_fn(actor_returns_result)).await;
    run_it(&mut context, runnable::boxed_from_fn(actor_exits)).await;

    run_it(
        &mut context,
        runnable::boxed_from_fn((actor_accepts_one_arg, ("named actor",))),
    )
    .await;

    run_it(
        &mut Context(Address::from_u64(10)),
        runnable::boxed_from_fn((actor_accepts_two_args, ("worker", 0))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(11)),
        runnable::boxed_from_fn((actor_accepts_two_args, ("worker", 1))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(12)),
        runnable::boxed_from_fn((actor_accepts_two_args, ("worker", 2))),
    )
    .await;
}
