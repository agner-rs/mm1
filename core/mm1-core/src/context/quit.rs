use mm1_common::types::{Never, StdError};

pub trait Quit: Send {
    fn quit<E>(&mut self, result: Result<(), E>) -> impl Future<Output = Never> + Send
    where
        E: StdError + Send + Sync + 'static,
    {
        async move {
            match result {
                Ok(()) => self.quit_ok().await,
                Err(reason) => self.quit_err(reason).await,
            }
        }
    }

    fn quit_ok(&mut self) -> impl Future<Output = Never> + Send;

    fn quit_err<E>(&mut self, reason: E) -> impl Future<Output = Never> + Send
    where
        E: StdError + Send + Sync + 'static;
}
