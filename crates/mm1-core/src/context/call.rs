use std::future::Future;

pub trait Call<To, Msg>: Send
where
    To: Send,
    Msg: Send,
{
    type Outcome: Send;

    fn call(&mut self, to: To, msg: Msg) -> impl Future<Output = Self::Outcome> + Send;
}

pub trait TryCall<To, Msg>: Call<To, Msg, Outcome = Result<Self::CallOk, Self::CallError>>
where
    To: Send,
    Msg: Send,
{
    type CallOk: Send;
    type CallError: Send;
}
impl<T, To, Msg, Ok, Err> TryCall<To, Msg> for T
where
    T: Call<To, Msg, Outcome = Result<Ok, Err>>,
    To: Send,
    Msg: Send,
    Ok: Send,
    Err: Send,
{
    type CallError = Err;
    type CallOk = Ok;
}
