// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

use std::fmt;

use mm1_address::address::Address;
use mm1_proto::message;

#[message(base_path = ::mm1_proto)]
pub struct RequestHeader {
    pub id:       u64,
    pub reply_to: Address,
}

#[message(base_path = ::mm1_proto)]
pub struct ResponseHeader {
    pub id: u64,
}

#[message(base_path = ::mm1_proto)]
pub struct Request<Rq> {
    pub header:  RequestHeader,
    pub payload: Rq,
}

#[message(base_path = ::mm1_proto)]
pub struct Response<Rs> {
    pub header:  ResponseHeader,
    pub payload: Rs,
}

impl fmt::Display for RequestHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "REQUEST({}!{})", self.reply_to, self.id)
    }
}
