pub mod ping_pong {
    use std::time::Duration;

    use mm1_address::address::Address;
    use mm1_core::context::{Ask, Fork, Recv, Tell};
    use mm1_core::envelope::dispatch;
    use mm1_proto::message;
    use tokio::time;

    #[derive(Debug)]
    #[message]
    pub struct Ping {
        reply_to: Address,
        seq_num:  u64,
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
        Ctx: Recv + Tell,
    {
        loop {
            let keep_running = dispatch!(match ctx.recv().await? {
                Ping { reply_to, seq_num } => {
                    let _ = ctx.tell(reply_to, Pong { seq_num }).await;
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
        Ctx: Recv + Tell + Fork,
    {
        for seq_num in 1..=(times as u64) {
            dispatch!(match time::timeout(
                timeout,
                ctx.ask(to, |reply_to| Ping { reply_to, seq_num }),
            )
            .await??
            {
                Pong { .. } => (),
            });
        }
        Ok(())
    }
}

fn main() {}
