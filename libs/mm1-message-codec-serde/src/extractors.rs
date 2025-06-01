use mm1_core::envelope::Envelope;
use mm1_message_codec::codec::RequiredType;

use crate::packet::Packet;

pub struct StandardExtractor;

impl<K, B> RequiredType<K, Packet<K, B>> for StandardExtractor
where
    K: Clone,
{
    fn required_type(&self, data: &Packet<K, B>) -> Option<K> {
        Some(data.type_key.clone())
    }
}

impl RequiredType<&'static str, Envelope> for StandardExtractor {
    fn required_type(&self, data: &Envelope) -> Option<&'static str> {
        Some(data.message_name())
    }
}
