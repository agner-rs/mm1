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

#[cfg(test)]
mod tests {
    use std::task::Waker;

    use super::*;

    #[test]
    fn non_string_panic_payload_has_a_useful_description() {
        let mut future = Box::pin(
            async {
                panic::panic_any(123_u32);
            }
            .catch_panic(),
        );
        let mut cx = Context::from_waker(Waker::noop());

        let Poll::Ready(Err(panic)) = future.as_mut().poll(&mut cx) else {
            panic!("future did not report its panic");
        };

        assert_eq!(&*panic, "<non-string panic payload>");
    }
}
