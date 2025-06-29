use messages::*;
use mm1_address::address::Address;
use mm1_core::envelope::{Envelope, EnvelopeHeader, dispatch};

pub mod messages {
    use mm1_proto::message;

    use crate::Address;

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct AUnit;

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct ATuple(pub String, pub String);

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct AStruct {
        pub s: String,
        pub i: i64,
    }

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Ping {
        pub reply_to: Address,
        pub seq_num:  u64,
    }

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Forward<Message> {
        pub forward_to: Address,
        pub message:    Message,
    }

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Pong {
        pub seq_num: u64,
    }
}

const ADDR: Address = Address::from_u64(111);

#[should_panic]
#[test]
fn test_01() {
    dispatch(
        Envelope::new(
            EnvelopeHeader::to_address(Address::from_u64(222)),
            Pong { seq_num: 0 },
        )
        .into_erased(),
    );
}

#[test]
fn test_02() {
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Ping {
                    reply_to: ADDR,
                    seq_num:  5,
                }
            )
            .into_erased()
        ),
        1
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Ping {
                    reply_to: ADDR,
                    seq_num:  15,
                }
            )
            .into_erased()
        ),
        2
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Ping {
                    reply_to: ADDR,
                    seq_num:  10,
                }
            )
            .into_erased()
        ),
        3
    );
}

#[test]
fn test_03() {
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::AUnit
            )
            .into_erased()
        ),
        4
    );
    // assert_eq!(dispatch(Inbound::new(INFO, (true, AUnit)).into_erased()), 5);
    // assert_eq!(dispatch(Inbound::new(INFO, (false, AUnit)).into_erased()),
    // 6);
}

#[test]
fn test_04() {
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::ATuple("1".into(), "2".into())
            )
            .into_erased()
        ),
        7
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::ATuple("2".into(), "1".into())
            )
            .into_erased()
        ),
        8
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::ATuple("_".into(), "_".into())
            )
            .into_erased()
        ),
        9
    );
}

#[test]
fn test_05() {
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::AStruct {
                    s: "1".into(),
                    i: 2,
                }
            )
            .into_erased()
        ),
        10
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::AStruct {
                    s: "2".into(),
                    i: 0,
                }
            )
            .into_erased()
        ),
        11
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                messages::AStruct {
                    s: "3".into(),
                    i: 3,
                }
            )
            .into_erased()
        ),
        12
    );
}

#[test]
fn test_06() {
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Forward {
                    forward_to: ADDR,
                    message:    Ping {
                        reply_to: ADDR,
                        seq_num:  1,
                    },
                }
            )
            .into_erased()
        ),
        13
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Forward {
                    forward_to: Address::from_u64(ADDR.into_u64().reverse_bits()),
                    message:    Ping {
                        reply_to: ADDR,
                        seq_num:  1,
                    },
                }
            )
            .into_erased()
        ),
        14
    );
    assert_eq!(
        dispatch(
            Envelope::new(
                EnvelopeHeader::to_address(Address::from_u64(222)),
                Forward {
                    forward_to: ADDR,
                    message:    Pong { seq_num: 1 },
                }
            )
            .into_erased()
        ),
        15
    );
}

fn dispatch(inbound: Envelope) -> usize {
    dispatch!(match inbound {
        ping @ Ping { seq_num, .. } if *seq_num < 10 => {
            eprintln!("{ping:?}");
            1
        },
        ping @ Ping { seq_num, .. } if *seq_num > 10 => {
            eprintln!("{ping:?}");
            2
        },
        ping @ Ping { .. } => {
            eprintln!("{ping:?}");
            3
        },

        AUnit => 4,

        // (true, AUnit) => 5,
        // (false, AUnit) => 6,
        ATuple(l, r) if l < r => 7,
        ATuple(l, r) if l == "2" => 8,
        ATuple { .. } => 9,

        AStruct { s, .. } if s == "1" => 10,
        AStruct { i, .. } if *i < 1 => 11,
        AStruct { .. } => 12,

        Forward::<Ping> {
            forward_to,
            message: _,
        } if *forward_to == ADDR => 13,
        Forward::<Ping> { .. } => 14,
        Forward::<Pong> { .. } => 15,
    })
}
