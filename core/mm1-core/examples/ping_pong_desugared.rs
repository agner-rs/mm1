pub mod ping_pong {

    use mm1_address::address::Address;
    use mm1_core::context::{Messaging, Tell};
    use mm1_proto::message;

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Ping {
        reply_to: Address,
        seq_num:  u64,
    }

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Forward<Message> {
        forward_to: Address,
        message:    Message,
    }

    #[derive(Debug)]
    #[message(base_path = ::mm1_proto)]
    pub struct Pong {
        #[allow(dead_code)]
        seq_num: u64,
    }

    pub async fn server<Ctx>(ctx: &mut Ctx) -> Result<(), eyre::Report>
    where
        Ctx: Messaging,
    {
        loop {
            let inbound = ctx.recv().await?;

            let ret_value_opt = 'handle: {
                let inbound = match inbound.cast::<Ping>() {
                    Ok(inbound) => {
                        let (Ping { reply_to, seq_num }, _) = inbound.take();
                        let _ = ctx.tell(reply_to, Pong { seq_num }).await;
                        break 'handle None
                    },
                    Err(inbound) => inbound,
                };

                let inbound = match inbound.cast::<Forward<Ping>>() {
                    Ok(inbound) => {
                        let (
                            Forward {
                                forward_to,
                                message,
                            },
                            _,
                        ) = inbound.take();
                        ctx.tell(forward_to, message)
                            .await
                            .expect("Heute leider nicht");

                        break 'handle None
                    },
                    Err(inbound) => inbound,
                };

                panic!("unexpected message: {inbound:?}")
            };

            if let Some(ret_value) = ret_value_opt {
                break ret_value;
            }
        }
    }
}

fn main() {}
