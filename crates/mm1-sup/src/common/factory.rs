use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::Mutex;

pub trait ActorFactory: Clone + Send + 'static {
    type Args;
    type Runnable;
    fn produce(&self, args: Self::Args) -> Self::Runnable;
}

#[derive(Debug)]
pub struct Func<F, A, R>(Arc<Mutex<F>>, PhantomData<(A, R)>);

impl<F, A, R> Func<F, A, R>
where
    F: FnMut(A) -> R,
    F: Send + 'static,
    A: Send + 'static,
    R: Send + 'static,
{
    pub fn new(f: F) -> Self {
        Self(Arc::new(Mutex::new(f)), Default::default())
    }
}

impl<F, A, R> Clone for Func<F, A, R> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), Default::default())
    }
}

impl<F, A, R> ActorFactory for Func<F, A, R>
where
    F: FnMut(A) -> R,
    F: Send + 'static,
    A: Send + 'static,
    R: Send + 'static,
{
    type Args = A;
    type Runnable = R;

    fn produce(&self, args: Self::Args) -> Self::Runnable {
        (self.0.lock())(args)
    }
}
