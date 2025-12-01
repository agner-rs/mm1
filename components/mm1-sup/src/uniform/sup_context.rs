use mm1_ask::Reply;
use mm1_core::context::{Fork, InitDone, Linking, Messaging, Quit, Start, Stop, Watching};

pub trait UniformSupContext<Runnable>:
    Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}
impl<Ctx, Runnable> UniformSupContext<Runnable> for Ctx where
    Ctx: Fork + InitDone + Linking + Messaging + Quit + Reply + Start<Runnable> + Stop + Watching
{
}
