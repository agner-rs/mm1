use insta::{assert_debug_snapshot, assert_yaml_snapshot};
use mm1_address::address::Address;
use mm1_core::envelope::{Envelope, EnvelopeHeader};
use mm1_message_codec::codec::Process;
use mm1_message_codec::compose::Dispatcher;
use serde_json::json;

use crate::extractors::StandardExtractor;
use crate::json::SerdeJson;
use crate::packet::Packet;

macro_rules! codec {
    ( $dispatcher: expr, < $( $t:ty ),* > ) => {
        $dispatcher
        $(
            .with(SerdeJson::<$t>::new()).unwrap()
        )*
    };
}

mod messages {
    use mm1_proc_macros::message;

    #[message]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub(crate) struct M1 {
        pub m_1: u32,
    }

    #[message]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub(crate) struct M2 {
        pub m_2: String,
    }

    #[message]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub(crate) struct M3 {
        pub m_3: serde_json::Value,
    }

    #[message]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub(crate) struct M4;
}

#[test]
fn macro_ergonomics() {
    let encoder = codec!(
        Dispatcher::<
            StandardExtractor,
            (),
            &'static str,
            Envelope,
            Packet<&'static str, String>,
            serde_json::Error,
        >::new(StandardExtractor), 
        < messages::M1, messages::M2, messages::M3 >);
    let m1 = Envelope::new(
        EnvelopeHeader::to_address(Address::from_u64(1)),
        messages::M1 { m_1: 1 },
    )
    .into_erased();
    let m2 = Envelope::new(
        EnvelopeHeader::to_address(Address::from_u64(2)),
        messages::M2 {
            m_2: "this is m2".into(),
        },
    )
    .into_erased();
    let m3 = Envelope::new(
        EnvelopeHeader::to_address(Address::from_u64(4)),
        messages::M3 {
            m_3: json!({"hello": "there"}),
        },
    )
    .into_erased();
    let m4 = Envelope::new(
        EnvelopeHeader::to_address(Address::from_u64(1)),
        messages::M4,
    )
    .into_erased();

    let mut packets = vec![];

    for envelope in [m1, m2, m3] {
        eprintln!("type-id: {:?}", envelope.tid());
        eprintln!("message-name: {}", envelope.message_name());
        let packet = encoder
            .process(&mut (), envelope)
            .unwrap()
            .into_result()
            .unwrap();
        packets.push(packet);
    }

    let _m4 = encoder
        .process(&mut (), m4)
        .unwrap()
        .into_result()
        .unwrap_err();

    assert_yaml_snapshot!("packets", packets);

    let decoder = codec!(
        Dispatcher::<
            StandardExtractor, 
            (), 
            String, 
            Packet<String, String>, 
            Envelope,
            serde_json::Error
        >::new(StandardExtractor),
        < messages::M1, messages::M2, messages::M3 >);

    let mut message_names = vec![];
    for packet in packets {
        let Packet {
            to,
            type_key,
            message,
        } = packet;
        let packet = Packet {
            to,
            type_key: type_key.to_owned(),
            message,
        };
        let envelope = decoder
            .process(&mut (), packet)
            .unwrap()
            .into_result()
            .unwrap();
        eprintln!("type-id: {:?}", envelope.tid());
        eprintln!("message-name: {}", envelope.message_name());
        message_names.push(envelope.message_name());
    }

    assert_debug_snapshot!("message_names", message_names);
}
