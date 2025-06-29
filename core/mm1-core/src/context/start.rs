use std::time::Duration;

use mm1_address::address::Address;
use mm1_common::errors::error_of::ErrorOf;
use mm1_proto_system::{SpawnErrorKind, StartErrorKind};

pub trait Start<Runnable> {
    fn spawn(
        &mut self,
        runnable: Runnable,
        link: bool,
    ) -> impl Future<Output = Result<Address, ErrorOf<SpawnErrorKind>>> + Send;

    fn start(
        &mut self,
        runnable: Runnable,
        link: bool,
        start_timeout: Duration,
    ) -> impl Future<Output = Result<Address, ErrorOf<StartErrorKind>>> + Send;
}

pub trait InitDone {
    fn init_done(&mut self, address: Address) -> impl Future<Output = ()> + Send;
}
