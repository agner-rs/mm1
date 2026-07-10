use mm1::address::Address;
use mm1::core::envelope::{Envelope, EnvelopeHeader, dispatch};
use mm1::proto::message;

#[message]
struct Ping;

fn main() {
    let envelope = Envelope::new(EnvelopeHeader::to_address(Address::from_u64(1)), Ping)
        .into_erased();

    dispatch!(match envelope {
        message => drop(message),
    });
}
