use std::future::Future;
use std::panic;
use std::pin::Pin;
use std::task::{Context, Poll};

type PanicInfo = Box<str>;

pub trait CatchPanicExt: Future + Sized {
    fn catch_panic(self) -> CatchPanic<Self> {
        CatchPanic(self)
    }
}

#[pin_project::pin_project]
pub struct CatchPanic<F>(#[pin] F);

impl<F> Future for CatchPanic<F>
where
    F: Future,
{
    type Output = Result<F::Output, PanicInfo>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let poll = panic::catch_unwind(panic::AssertUnwindSafe(|| this.0.poll(cx)));

        match poll {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Err(panic_info) => {
                let s: Box<str> = if let Some(s) = panic_info.downcast_ref::<&'static str>() {
                    s.to_owned().into()
                } else if let Ok(s) = panic_info.downcast::<String>() {
                    (*s).into()
                } else {
                    String::new().into()
                };
                Poll::Ready(Err(s))
            },
        }
    }
}

impl<F> CatchPanicExt for F where Self: Future + Sized {}
