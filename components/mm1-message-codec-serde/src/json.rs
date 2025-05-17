use std::marker::PhantomData;

use mm1_core::envelope::{Envelope, EnvelopeInfo};
use mm1_core::prim::Message;
use mm1_message_codec::codec::{Outcome, Process, SupportedTypes};

use crate::packet::Packet;

#[derive(Debug, Clone, Copy)]
pub struct SerdeJson<M>(PhantomData<M>);

impl<M> SerdeJson<M> {
    pub fn new() -> Self {
        Default::default()
    }
}

impl<M> Default for SerdeJson<M> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<M> SupportedTypes<String> for SerdeJson<M> {
    fn supported_types(&self) -> impl Iterator<Item = String> {
        [std::any::type_name::<M>().into()].into_iter()
    }
}

impl<M> SupportedTypes<&'static str> for SerdeJson<M> {
    fn supported_types(&self) -> impl Iterator<Item = &'static str> {
        [std::any::type_name::<M>()].into_iter()
    }
}

impl<M> Process<Envelope> for SerdeJson<M>
where
    M: Message + serde::Serialize,
{
    type Error = serde_json::Error;
    type Output = Packet<&'static str, String>;
    type State = ();

    fn process(
        &self,
        _state: &mut Self::State,
        envelope: Envelope,
    ) -> Result<Outcome<Self::Output, Envelope>, Self::Error> {
        let type_key = envelope.message_name();
        let (message, envelope) = match envelope.cast::<M>() {
            Ok(m) => m.take(),
            Err(e) => return Ok(Outcome::Rejected(e)),
        };
        let EnvelopeInfo { to, .. } = envelope.info();
        let message = serde_json::to_string(&message)?;
        let output = Packet {
            to: *to,
            type_key,
            message,
        };

        Ok(Outcome::Done(output))
    }
}

impl<S, B, M> Process<Packet<S, B>> for SerdeJson<M>
where
    M: Message + serde::de::DeserializeOwned,
    S: AsRef<str>,
    B: AsRef<[u8]>,
{
    type Error = serde_json::Error;
    type Output = Envelope;
    type State = ();

    fn process(
        &self,
        _state: &mut Self::State,
        packet: Packet<S, B>,
    ) -> Result<Outcome<Self::Output, Packet<S, B>>, Self::Error> {
        if std::any::type_name::<M>() != packet.type_key.as_ref() {
            return Ok(Outcome::Rejected(packet))
        }

        let message: M = serde_json::from_slice::<M>(packet.message.as_ref())?;
        let info = EnvelopeInfo::new(packet.to);
        let envelope = Envelope::new(info, message).into_erased();

        Ok(Outcome::Done(envelope))
    }
}

#[cfg(test)]
mod tests;
