//! The handlers' operation-status protocol (`UdsOpStatus`): a handler returning
//! response-pending is called again on each poll until it settles, and told to cancel
//! once if the request is dropped meanwhile.

use core::future::poll_fn;
use core::task::Poll;

/// `UDS_OP_INITIAL`.
const INITIAL: u8 = 0x00;
/// `UDS_OP_PENDING`.
const PENDING: u8 = 0x01;
/// `UDS_OP_CANCEL`.
const CANCEL: u8 = 0x02;

/// `UDS_NRC_POSITIVE_RESPONSE`.
pub const POSITIVE_RESPONSE: u8 = 0x00;
/// `UDS_NRC_RESPONSE_PENDING`.
pub const RESPONSE_PENDING: u8 = 0x78;

/// `UDS_NRC_INVALID_KEY`: the key handler's verdict on a wrong key.
pub const INVALID_KEY: u8 = 0x35;

/// One call of a C handler, `call(op) -> nrc`, until it settles.
struct OpCall<F: FnMut(u8) -> u8> {
    call: F,
    pending: bool,
}

impl<F: FnMut(u8) -> u8> Drop for OpCall<F> {
    fn drop(&mut self) {
        if self.pending {
            let _ = (self.call)(CANCEL);
        }
    }
}

/// Calls `call` with INITIAL, then with PENDING on each later poll while it
/// answers response-pending; resolves to the code it settles on. Dropped
/// while pending, it calls `call` once more with CANCEL.
///
/// It returns `Pending` without registering a waker: the firmware polls the server on
/// every pass of its loop, and that next poll is what calls the handler again.
pub async fn settle(call: impl FnMut(u8) -> u8) -> u8 {
    let mut op = OpCall {
        call,
        pending: false,
    };
    poll_fn(|_| {
        let status = if op.pending { PENDING } else { INITIAL };
        match (op.call)(status) {
            RESPONSE_PENDING => {
                op.pending = true;
                Poll::Pending
            }
            nrc => {
                op.pending = false;
                Poll::Ready(nrc)
            }
        }
    })
    .await
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test harness: a handler records at most its fixed number of calls"
)]
mod tests {
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};

    use super::*;

    /// A handler that answers response-pending `pends` times, then `nrc`,
    /// recording every status it was called with.
    struct Handler {
        pends: usize,
        nrc: u8,
        calls: [u8; 8],
        count: usize,
    }

    impl Handler {
        fn new(pends: usize, nrc: u8) -> Self {
            Self {
                pends,
                nrc,
                calls: [0xFF; 8],
                count: 0,
            }
        }

        fn call(&mut self, status: u8) -> u8 {
            self.calls[self.count] = status;
            self.count += 1;
            if self.count <= self.pends {
                RESPONSE_PENDING
            } else {
                self.nrc
            }
        }

        fn calls(&self) -> &[u8] {
            &self.calls[..self.count]
        }
    }

    fn poll<F: Future>(f: core::pin::Pin<&mut F>) -> Poll<F::Output> {
        f.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn a_handler_that_settles_at_once_is_called_initial_only() {
        let mut handler = Handler::new(0, POSITIVE_RESPONSE);
        assert_eq!(
            poll(pin!(settle(|s| handler.call(s)))),
            Poll::Ready(POSITIVE_RESPONSE)
        );
        assert_eq!(handler.calls(), [INITIAL]);
    }

    #[test]
    fn a_pending_handler_is_called_again_with_pending_until_it_settles() {
        let mut handler = Handler::new(2, 0x22);
        {
            let mut call = pin!(settle(|s| handler.call(s)));
            assert_eq!(poll(call.as_mut()), Poll::Pending);
            assert_eq!(poll(call.as_mut()), Poll::Pending);
            assert_eq!(poll(call.as_mut()), Poll::Ready(0x22));
        }
        assert_eq!(handler.calls(), [INITIAL, PENDING, PENDING]);
    }

    #[test]
    fn a_pending_handler_dropped_unsettled_is_told_to_cancel() {
        let mut handler = Handler::new(5, POSITIVE_RESPONSE);
        {
            let mut call = pin!(settle(|s| handler.call(s)));
            assert_eq!(poll(call.as_mut()), Poll::Pending);
        }
        assert_eq!(handler.calls(), [INITIAL, CANCEL]);
    }

    #[test]
    fn a_settled_handler_is_not_canceled_on_drop() {
        let mut handler = Handler::new(1, POSITIVE_RESPONSE);
        {
            let mut call = pin!(settle(|s| handler.call(s)));
            assert_eq!(poll(call.as_mut()), Poll::Pending);
            assert_eq!(poll(call.as_mut()), Poll::Ready(POSITIVE_RESPONSE));
        }
        assert_eq!(handler.calls(), [INITIAL, PENDING]);
    }
}
