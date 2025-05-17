#[derive(Debug)]
pub enum Outcome<Output, Input> {
    Done(Output),
    Rejected(Input),
}

pub trait Process<Input> {
    type State;
    type Output;
    type Error;

    fn process(
        &self,
        state: &mut Self::State,
        input: Input,
    ) -> Result<Outcome<Self::Output, Input>, Self::Error>;
}

pub trait SupportedTypes<K> {
    fn supported_types(&self) -> impl Iterator<Item = K>;
}

pub trait RequiredType<K, I> {
    fn required_type(&self, data: &I) -> Option<K>;
}

impl<Output, Input> Outcome<Output, Input> {
    pub fn into_result(self) -> Result<Output, Input> {
        match self {
            Self::Done(output) => Ok(output),
            Self::Rejected(input) => Err(input),
        }
    }
}
