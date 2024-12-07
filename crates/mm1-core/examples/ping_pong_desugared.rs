pub mod ping_pong {

    use mm1_address::address::Address;
    use mm1_core::context::{Recv, Tell};
    use mm1_proto::Traversable;

    #[derive(Traversable)]
    pub struct Ping {
        reply_to: Address,
        seq_num:  u64,
    }

    #[derive(Traversable)]
    pub struct Forward<Message> {
        forward_to: Address,
        message:    Message,
    }

    #[derive(Traversable)]
    pub struct Pong {
        seq_num: u64,
    }

    pub async fn server<Ctx>(ctx: &mut Ctx) -> Result<(), eyre::Report>
    where
        Ctx: Tell + Recv,
    {
        loop {
            let inbound = ctx.recv().await?;

            let ret_value_opt = 'handle: {
                let inbound = match inbound.cast::<Ping>() {
                    Ok(inbound) => {
                        break 'handle ({
                            let (Ping { reply_to, seq_num }, _) = inbound.take_message();
                            let _ = ctx.tell(reply_to, Pong { seq_num }).await;

                            None
                        })
                    },
                    Err(inbound) => inbound,
                };

                let inbound = match inbound.cast::<Forward<Ping>>() {
                    Ok(inbound) => {
                        break 'handle ({
                            let (
                                Forward {
                                    forward_to,
                                    message,
                                },
                                _,
                            ) = inbound.take_message();
                            ctx.tell(forward_to, message)
                                .await
                                .expect("Heute leider nicht");

                            None
                        })
                    },
                    Err(inbound) => inbound,
                };

                panic!("unexpected message: {:?}", inbound)
            };

            if let Some(ret_value) = ret_value_opt {
                break ret_value;
            }
        }
    }
}

fn main() {}
