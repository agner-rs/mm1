// use mm1_address::address::Address;
// use mm1_proto::message;
// use mm1_proto_system::{StartErrorKind, StopErrorKind};

// #[derive(Debug)]
// #[message(base_path = ::mm1_proto)]
// pub struct StartRequest<Key> {
//     pub reply_to: Address,
//     pub child_id: Key,
// }

// pub type StartResponse = Result<(), StartErrorKind>;

// #[derive(Debug)]
// #[message(base_path = ::mm1_proto)]
// pub struct StopRequest<Key> {
//     pub reply_to: Address,
//     pub child_id: Key,
// }

// pub type StopResponse = Result<(), StopErrorKind>;
