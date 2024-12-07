use mm1_address::address::Address;
use mm1_common::errors::error_kind::HasErrorKind;
use mm1_core::context::{Call, Fork, TryCall};
use mm1_proto_system::{InitAck, Kill, SpawnErrorKind, SpawnRequest, System};

use crate::mixed::MixedSup;

pub async fn main<Sys, Ctx, RS, C>(ctx: &mut Ctx, sup_spec: MixedSup<RS, C>)
where
    Sys: System + Default,
    Ctx: Fork,
    Ctx: Call<Sys, InitAck, Outcome = ()>,
    Ctx: Call<Sys, Kill, Outcome = bool>,
    Ctx: TryCall<Sys, SpawnRequest<Sys>, CallOk = Address>,
    <Ctx as TryCall<Sys, SpawnRequest<Sys>>>::CallError: HasErrorKind<SpawnErrorKind>,
{
    let _ = (ctx, sup_spec);
}
