pub mod ping_pong {
    use std::time::Duration;

    use mm1::address::Address;
    use mm1::ask::proto::Request;
    use mm1::ask::{Ask, Reply};
    use mm1::core::context::{Fork, Messaging, Tell};
    use mm1::core::envelope::dispatch;
    use mm1::proto::message;

    #[derive(Debug)]
    #[message]
    pub struct Ping {
        seq_num: u64,
    }

    #[derive(Debug)]
    #[message]
    pub struct Forward<Message> {
        forward_to: Address,
        message:    Message,
    }

    #[derive(Debug)]
    #[message]
    pub struct Pong {
        #[allow(dead_code)]
        seq_num: u64,
    }

    pub async fn server<Ctx>(ctx: &mut Ctx) -> Result<(), eyre::Report>
    where
        Ctx: Messaging + Reply,
    {
        loop {
            let keep_running = dispatch!(match ctx.recv().await? {
                Request::<_> {
                    header: reply_to,
                    payload: Ping { seq_num },
                } => {
                    let _ = ctx.reply(reply_to, Pong { seq_num }).await;
                    true
                },
                Forward::<Ping> {
                    forward_to,
                    message,
                } => {
                    ctx.tell(forward_to, message)
                        .await
                        .expect("Heute leider nicht");
                    true
                },
            });

            if !keep_running {
                break Ok(());
            }
        }
    }

    pub async fn client<Ctx>(
        ctx: &mut Ctx,
        to: Address,
        times: usize,
        timeout: Duration,
    ) -> Result<(), eyre::Report>
    where
        Ctx: Messaging + Fork,
    {
        for seq_num in 1..=(times as u64) {
            let Pong { .. } = ctx.ask(to, Ping { seq_num }, timeout).await?;
        }
        Ok(())
    }
}

fn main() {}
