use std::fmt;

use mm1_address::address::Address;
use mm1_proto::message;

#[message]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RequestHeader<Id> {
    pub id:       Id,
    pub reply_to: Address,
}

#[message]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResponseHeader<Id> {
    pub id: Id,
}

#[message]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Request<Rq, Id = ()> {
    pub header:  RequestHeader<Id>,
    pub payload: Rq,
}

#[message]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Response<Rs, Id = ()> {
    pub header:  ResponseHeader<Id>,
    pub payload: Rs,
}

impl<Id> fmt::Display for RequestHeader<Id>
where
    Id: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "REQUEST({}!{:?})", self.reply_to, self.id)
    }
}
