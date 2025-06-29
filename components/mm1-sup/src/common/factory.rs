use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::Mutex;

pub trait ActorFactory: Send + Sync + 'static {
    type Args: Send + 'static;
    type Runnable;
    fn produce(&self, args: Self::Args) -> Self::Runnable;
}

#[derive(Debug)]
pub struct ActorFactoryMut<F, A, R>(Arc<Mutex<F>>, PhantomData<(A, R)>);

#[derive(Debug)]
pub struct ActorFactoryOnce<F, A, R>(Arc<Mutex<Option<F>>>, PhantomData<(A, R)>);

impl<F, A, R> ActorFactoryMut<F, A, R>
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

impl<F, A, R> ActorFactoryOnce<F, A, R>
where
    F: FnOnce(A) -> R,
    F: Send + 'static,
    A: Send + 'static,
    R: Send + 'static,
{
    pub fn new(f: F) -> Self {
        Self(Arc::new(Mutex::new(Some(f))), Default::default())
    }
}

impl<F, A, R> Clone for ActorFactoryMut<F, A, R> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), Default::default())
    }
}

impl<F, A, R> Clone for ActorFactoryOnce<F, A, R> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), Default::default())
    }
}

impl<F, A, R> ActorFactory for ActorFactoryMut<F, A, R>
where
    Self: Sync + Send + 'static,
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

impl<F, A, R> ActorFactory for ActorFactoryOnce<F, A, R>
where
    Self: Sync + Send + 'static,
    F: FnOnce(A) -> R,
    F: Send + 'static,
    // A: Message,
    A: Send + 'static,
    R: Send + 'static,
{
    type Args = A;
    type Runnable = R;

    fn produce(&self, args: Self::Args) -> Self::Runnable {
        (self
            .0
            .lock()
            .take()
            .expect("this is a single-use actor-factory"))(args)
    }
}
