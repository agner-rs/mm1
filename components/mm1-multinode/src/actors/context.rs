use mm1_address::subnet::NetAddress;
use mm1_ask::{Ask, Reply};
use mm1_core::context::{
    Bind, Fork, InitDone, Linking, Messaging, Now, Quit, Start, Stop, Tell, Watching,
};
use mm1_runnable::local;
use tokio::time::Instant;

trait_set::trait_set! {
    pub trait ActorContext =
        Ask +
        Bind<NetAddress> +
        Fork +
        InitDone +
        Linking +
        Messaging +
        Now<Instant = Instant> +
        Reply +
        Start<local::BoxedRunnable<Self>> +
        Stop +
        Tell +
        Quit +
        Watching +
        Sized +
        Send +
        Sync +
        'static;
}
