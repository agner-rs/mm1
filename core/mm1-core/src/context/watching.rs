use mm1_address::address::Address;
use mm1_proto_system as system;

pub trait Watching {
    fn watch(&mut self, peer: Address) -> impl Future<Output = system::WatchRef> + Send;
    fn unwatch(&mut self, watch_ref: system::WatchRef) -> impl Future<Output = ()> + Send;
}
