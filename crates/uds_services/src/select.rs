//! Waiting on two futures at once, in `core` alone.
//!
//! ``UDSSVC_ARCH_0016`` promises that a handler outrunning `tP2_Server` yields and the
//! driver submits a response-pending in that window. Reaching that window means waiting
//! on "the handler completes **or** the deadline passes", and ``UDSSVC_ARCH_0030``
//! forbids depending on a runtime, so this crate writes its own. It is the only
//! machinery here that is not UDS.
//!
//! The `Unpin` bounds keep it free of `unsafe`. An `async fn` future is `!Unpin`, but
//! `Pin<&mut F>` is always `Unpin` and is a `Future` when `F` is, so a caller wraps each
//! side in [`core::pin::pin!`] and the bound is met safely. The driver additionally uses
//! `Pin::as_mut` so one handler survives several deadlines.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Which of two futures completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Either<A, B> {
    /// The first completed.
    Left(A),
    /// The second completed.
    Right(B),
}

/// The future [`select2`] returns.
#[derive(Debug)]
#[must_use = "a select does nothing until it is awaited"]
pub struct Select2<F, G> {
    first: F,
    second: G,
}

impl<F: Future + Unpin, G: Future + Unpin> Future for Select2<F, G> {
    type Output = Either<F::Output, G::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        if let Poll::Ready(value) = Pin::new(&mut this.first).poll(cx) {
            return Poll::Ready(Either::Left(value));
        }
        if let Poll::Ready(value) = Pin::new(&mut this.second).poll(cx) {
            return Poll::Ready(Either::Right(value));
        }
        Poll::Pending
    }
}

/// Complete when either future does, reporting which.
///
/// `first` is polled first, so it wins a tie. The driver passes the handler as `first`
/// deliberately: a handler that finished in the same wake as the deadline has a final
/// response to send, and a response-pending for work already done is a message the
/// standard does not ask for.
pub fn select2<F: Future + Unpin, G: Future + Unpin>(first: F, second: G) -> Select2<F, G> {
    Select2 { first, second }
}

#[cfg(test)]
#[allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test doubles assert invariants that only ever hold in this module"
)]
mod tests {
    use super::{Either, select2};
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};

    struct Ready<T>(Option<T>);
    impl<T: Unpin> Future for Ready<T> {
        type Output = T;
        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
            Poll::Ready(self.0.take().expect("polled after completion"))
        }
    }

    struct Never;
    impl Future for Never {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            Poll::Pending
        }
    }

    fn cx_with(waker: &Waker) -> Context<'_> {
        Context::from_waker(waker)
    }

    /// The handler completing wins.
    #[test]
    fn a_ready_left_future_completes_the_select() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let mut s = select2(Ready(Some(7_u8)), Never);
        assert!(matches!(
            Pin::new(&mut s).poll(&mut cx),
            Poll::Ready(Either::Left(7))
        ));
    }

    /// The deadline passing wins when the handler is still running.
    #[test]
    fn a_ready_right_future_completes_the_select() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let mut s = select2(Never, Ready(Some(())));
        assert!(matches!(
            Pin::new(&mut s).poll(&mut cx),
            Poll::Ready(Either::Right(()))
        ));
    }

    /// Left is polled first, so it wins a tie — deliberately. A handler that finished
    /// in the same wake as the deadline has a final response to send, and a
    /// response-pending for work already done is a message nothing asked for.
    #[test]
    fn the_left_future_wins_a_tie() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let mut s = select2(Ready(Some(1_u8)), Ready(Some(())));
        assert!(matches!(
            Pin::new(&mut s).poll(&mut cx),
            Poll::Ready(Either::Left(1))
        ));
    }

    /// Neither ready means pending.
    #[test]
    fn two_pending_futures_are_pending() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let mut s = select2(Never, Never);
        assert!(Pin::new(&mut s).poll(&mut cx).is_pending());
    }

    /// An `async fn` future is `!Unpin`; `core::pin::pin!` is what satisfies the bound
    /// without `unsafe`. This is the shape the driver uses, so it is compiled here.
    #[test]
    fn a_pinned_async_block_satisfies_the_unpin_bound() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let handler = core::pin::pin!(async { 0x22_u8 });
        let deadline = core::pin::pin!(async {});
        let mut s = select2(handler, deadline);
        assert!(matches!(
            Pin::new(&mut s).poll(&mut cx),
            Poll::Ready(Either::Left(0x22))
        ));
    }

    /// The driver polls one handler across several deadlines, so the left future must
    /// survive being selected on more than once. `Pin::as_mut` is how.
    #[test]
    fn the_left_future_survives_repeated_selection() {
        let waker = Waker::noop();
        let mut cx = cx_with(waker);
        let mut handler = core::pin::pin!(async { 9_u8 });
        for _ in 0..2 {
            let mut s = select2(handler.as_mut(), Never);
            if let Poll::Ready(Either::Left(v)) = Pin::new(&mut s).poll(&mut cx) {
                assert_eq!(v, 9);
                return;
            }
        }
        panic!("the handler must complete");
    }
}
