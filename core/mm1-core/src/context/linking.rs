use mm1_address::address::Address;

pub trait Linking {
    fn link(&mut self, peer: Address) -> impl Future<Output = ()> + Send;
    fn unlink(&mut self, peer: Address) -> impl Future<Output = ()> + Send;
    fn set_trap_exit(&mut self, enable: bool) -> impl Future<Output = ()> + Send;
}
