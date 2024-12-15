use mm1_core::context::{Fork, InitDone, Linking, Quit, Recv, Start, Stop, Tell, Watching};
use mm1_proto_system::System;

use crate::mixed::MixedSup;

pub async fn main<Sys, Ctx, RS, C>(ctx: &mut Ctx, sup_spec: MixedSup<RS, C>)
where
    Sys: System + Default,

    Ctx: Fork + Recv + Tell + Quit,
    Ctx: InitDone<Sys>,
    Ctx: Linking<Sys>,
    Ctx: Watching<Sys>,
    Ctx: Start<Sys>,
    Ctx: Stop<Sys>,
{
    let _ = (ctx, sup_spec);
}
