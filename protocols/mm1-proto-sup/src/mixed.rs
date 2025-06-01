// use mm1_address::address::Address;
// use mm1_proto::message;
// use mm1_proto_system::{StartErrorKind, StopErrorKind};

// #[derive(Debug)]
// #[message]
// pub struct StartRequest<Key> {
//     pub reply_to: Address,
//     pub child_id: Key,
// }

// pub type StartResponse = Result<(), StartErrorKind>;

// #[derive(Debug)]
// #[message]
// pub struct StopRequest<Key> {
//     pub reply_to: Address,
//     pub child_id: Key,
// }

// pub type StopResponse = Result<(), StopErrorKind>;
