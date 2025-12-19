use std::fmt;

use mm1_address::address::Address;

use crate::envelope::Envelope;

pub trait MessageTap: fmt::Debug + Send + Sync + 'static {
    fn on_send(&self, sender: Address, to: Address, envelope: &Envelope);
    fn on_recv(&self, receiver: Address, envelope: &Envelope);
}

#[derive(Debug)]
pub struct NoopTap;

impl MessageTap for NoopTap {
    fn on_send(&self, _sender: Address, _to: Address, _envelope: &Envelope) {}

    fn on_recv(&self, _receiver: Address, _envelope: &Envelope) {}
}
