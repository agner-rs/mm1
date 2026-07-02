// `#[message]` on plain, tuple, and unit structs must compile.
use mm1::address::Address;
use mm1::proto::message;

#[message]
struct Ping {
    reply_to: Address,
    seq:      u64,
}

#[message]
struct Wrap(pub u64, pub Vec<u8>);

#[message]
struct Unit;

fn assert_message<T: mm1::proto::Message>() {}

fn main() {
    assert_message::<Ping>();
    assert_message::<Wrap>();
    assert_message::<Unit>();
}
