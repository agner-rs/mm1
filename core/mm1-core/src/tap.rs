use std::fmt;

use mm1_address::address::Address;
use mm1_address::subnet::NetAddress;

use crate::envelope::Envelope;

pub struct OnSend<'a> {
    pub sent_by_addr: Address,
    pub sent_by_net:  NetAddress,
    pub sent_by_key:  &'a dyn fmt::Display,
    pub sent_to_addr: Address,
    pub envelope:     &'a Envelope,
}

pub struct OnRecv<'a> {
    pub recv_by_addr: Address,
    pub recv_by_net:  NetAddress,
    pub recv_by_key:  &'a dyn fmt::Display,
    pub envelope:     &'a Envelope,
}

pub trait MessageTap: fmt::Debug + Send + Sync + 'static {
    fn on_send(&self, event: OnSend<'_>);
    fn on_recv(&self, event: OnRecv<'_>);
}

#[derive(Debug)]
pub struct NoopTap;

impl MessageTap for NoopTap {
    fn on_send(&self, _event: OnSend<'_>) {}

    fn on_recv(&self, _event: OnRecv<'_>) {}
}
