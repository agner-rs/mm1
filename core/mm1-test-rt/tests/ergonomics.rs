use mm1_address::address::Address;
use mm1_core::context::{Quit, Recv, Tell};
use mm1_core::envelope::dispatch;
use mm1_core::prim::message;
use mm1_test_rt::query::{self, Query};
use mm1_test_rt::runtime::{Event, TestRuntime};

#[tokio::test]
async fn ergonomics_quitter() {
    let _ = mm1_logger::init(&logger_config());

    let mut test_runtime = TestRuntime::<&'static str>::new()
        .with_actor(Address::from_u64(1), quitter)
        .expect("with_actor");

    let Some(Event::Query {
        address: address_that_asked_to_quit,
        query: Query::QuitOk(_),
    }) = test_runtime.next_event().await
    else {
        panic!()
    };
    assert!(test_runtime.rm_actor(address_that_asked_to_quit));
    let Some(Event::Done {
        address: address_that_terminated,
    }) = test_runtime.next_event().await
    else {
        panic!()
    };
    assert_eq!(address_that_asked_to_quit, address_that_terminated);
}

async fn quitter<Ctx>(ctx: &mut Ctx)
where
    Ctx: Quit,
{
    ctx.quit_ok().await;
}

#[tokio::test]
async fn ergonomics_sender() {
    let _ = mm1_logger::init(&logger_config());

    let mut test_runtime = TestRuntime::<&'static str>::new()
        .with_actor(Address::from_u64(123456789), sender)
        .expect("with_actor");

    let Some(Event::Query { address, query }) = test_runtime.next_event().await else {
        panic!()
    };
    eprintln!("[{}] {:?}", address, query);
    let Query::Tell(query::Tell {
        envelope,
        outcome_tx,
    }) = query
    else {
        panic!()
    };
    outcome_tx.send(Ok(())).expect("outcome_tx.send");

    let Some(Event::Query { address, query }) = test_runtime.next_event().await else {
        panic!()
    };
    eprintln!("[{}] {:?}", address, query);
    let Query::Recv(query::Recv { outcome_tx }) = query else {
        panic!()
    };
    outcome_tx.send(Ok(envelope)).expect("outcome_tx.send");
}

async fn sender<Ctx>(ctx: &mut Ctx)
where
    Ctx: Recv + Tell + Quit,
{
    #[message]
    struct Hello;

    let own_address = ctx.address();
    ctx.tell(own_address, Hello).await.expect("tell");
    let hello = ctx.recv().await.expect("recv");
    dispatch!(match hello {
        Hello => ctx.quit_ok().await,
    });
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "mm1_sup::*=DEBUG".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
