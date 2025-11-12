use std::time::Duration;

use mm1::address::Address;
use mm1::common::Never;
use mm1::common::error::{AnyError, ErrorOf, StdError};
use mm1::core::context::*;
use mm1::core::envelope::Envelope;
use mm1::runnable::local::{self, ActorRun};
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
    eprintln!("my name is {name}");
}
async fn actor_accepts_two_args<C>(_context: &mut C, name: &str, idx: usize) {
    eprintln!("my name is {name}[{idx}]");
}

async fn run_it<C, A>(context: &mut C, actor: A)
where
    C: Quit + Messaging,
    A: ActorRun<C>,
{
    time::timeout(Duration::from_millis(1), actor.run(context))
        .await
        .expect_err("should timeout");
    eprintln!("Exited. Address: {}", context.address());
}

struct Context(Address);

impl Messaging for Context {
    fn address(&self) -> Address {
        self.0
    }

    async fn recv(&mut self) -> Result<Envelope, ErrorOf<RecvErrorKind>> {
        Err(ErrorOf::new(RecvErrorKind::Closed, ""))
    }

    async fn close(&mut self) {}

    async fn send(&mut self, _envelope: Envelope) -> Result<(), ErrorOf<SendErrorKind>> {
        Ok(())
    }

    async fn forward(
        &mut self,
        _to: Address,
        _envelope: Envelope,
    ) -> Result<(), ErrorOf<SendErrorKind>> {
        Ok(())
    }
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

    run_it(&mut context, local::from_fn(actor_returns_unit)).await;
    run_it(&mut context, local::from_fn(actor_returns_result)).await;
    run_it(&mut context, local::from_fn(actor_returns_any_error)).await;
    run_it(&mut context, local::from_fn(actor_exits)).await;

    run_it(
        &mut context,
        local::from_fn((actor_accepts_one_arg, ("named actor",))),
    )
    .await;

    run_it(
        &mut Context(Address::from_u64(10)),
        local::from_fn((actor_accepts_two_args, ("worker", 0))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(11)),
        local::from_fn((actor_accepts_two_args, ("worker", 1))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(12)),
        local::from_fn((actor_accepts_two_args, ("worker", 2))),
    )
    .await;
}

#[tokio::test]
async fn boxed_from_fn() {
    let mut context = Context(Address::from_u64(1));

    run_it(&mut context, local::boxed_from_fn(actor_returns_unit)).await;
    run_it(&mut context, local::boxed_from_fn(actor_returns_result)).await;
    run_it(&mut context, local::boxed_from_fn(actor_exits)).await;

    run_it(
        &mut context,
        local::boxed_from_fn((actor_accepts_one_arg, ("named actor",))),
    )
    .await;

    run_it(
        &mut Context(Address::from_u64(10)),
        local::boxed_from_fn((actor_accepts_two_args, ("worker", 0))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(11)),
        local::boxed_from_fn((actor_accepts_two_args, ("worker", 1))),
    )
    .await;
    run_it(
        &mut Context(Address::from_u64(12)),
        local::boxed_from_fn((actor_accepts_two_args, ("worker", 2))),
    )
    .await;
}
