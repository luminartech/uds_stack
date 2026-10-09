//! The fixtures every driver test shares: one scripted transport, a future
//! that pends a set number of times, and an executor for futures that finish in bounded
//! polls.

#![allow(
    dead_code,
    reason = "each test crate compiles this module on its own and uses a different part"
)]
#![allow(
    clippy::panic,
    reason = "test harness: an overflowing script or a stuck future fails the test loudly"
)]

use core::future::{Future, poll_fn};
use core::task::Poll;
use uds_services::{
    Address, AfterSend, Ai, ClientTransport, Reloads, SResult, Timestamp, TransportEvent,
    UdsTransport,
};

/// One scripted step: what `next_event` yields, and the clock it yields it at.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// A message arrives.
    Ind(Ai, &'static [u8]),
    /// A message arrives with the clock already at this instant: the coinciding case.
    IndAt(u32, Ai, &'static [u8]),
    /// A message longer than the buffer offered arrives: what fits, and its length.
    TooLong(Ai, &'static [u8]),
    /// The connection to this peer fails or is given up.
    Close(Address),
    /// The connection to this peer closes as the standard prescribes, after a positive
    /// `DiagnosticSessionControl` or `ECUReset` response.
    Leave(Address),
    /// The transmission to this addressing completes with this result.
    Conf(Ai, SResult),
    /// Advance the clock. `next_event` reports `Deadline` only if that reaches the
    /// deadline the driver asked for; otherwise the time passes with nothing to report,
    /// and the next step is taken, as a real transport would go on waiting.
    At(u32),
    /// Advance the clock and take the next step, the deadline unreported: what arrives
    /// next arrives at or after it.
    Slip(u32),
    /// `next_event` pends once, as a transport with nothing to report does, so a caller
    /// can drop the future that awaits it.
    Pend,
}

/// The most steps a script can have.
pub const STEPS: usize = 32;
const SENT: usize = 16;
const FRAME: usize = 32;

/// One recorded transmission.
#[derive(Debug, Clone, Copy)]
pub struct Sent {
    /// Its addressing.
    pub ai: Ai,
    bytes: [u8; FRAME],
    len: usize,
    /// The clock when it was submitted.
    pub at: u32,
    /// How many script steps had been consumed when it was submitted.
    pub after: usize,
    /// What it said follows it.
    pub then: AfterSend,
}

impl Sent {
    /// Its bytes.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}

/// A transport that plays a fixed script and records what was sent.
///
/// It panics rather than erring on overflow: a driver reads a transport error as the end
/// of the script, so an error here would end a run silently and hide its cause.
#[derive(Debug)]
pub struct Script {
    steps: [Option<Step>; STEPS],
    /// How many steps the script has.
    pub len: usize,
    /// How many steps have been taken.
    pub cursor: usize,
    now: u32,
    sent: [Option<Sent>; SENT],
    /// How many transmissions were made.
    pub sent_count: usize,
    /// How many `Deadline`s were reported: one per `At` that reached the driver's deadline.
    pub deadlines: usize,
    /// Which addressing a transmission may carry; anything else fails the test.
    may_send: fn(Ai) -> bool,
    reloads: Reloads,
    /// How many transmissions were asked for, refused ones included.
    asked: usize,
    /// Which of them `t_data_req` refuses with `Err`, recording nothing.
    refused: Option<usize>,
    /// Which of them `t_data_req` records and then pends once on, so a caller can drop
    /// the future awaiting a transmission the transport has taken.
    stalled: Option<usize>,
    /// How far the clock moves while a transmission is handed over.
    send_cost: u32,
    /// How many times the client closed it.
    pub closes: usize,
}

impl Script {
    /// A transport playing `script`, accepting a transmission to any addressing.
    pub fn new(script: &[Step]) -> Self {
        assert!(script.len() <= STEPS, "script has more than {STEPS} steps");
        let mut steps = [None; STEPS];
        for (slot, step) in steps.iter_mut().zip(script) {
            *slot = Some(*step);
        }
        Self {
            steps,
            len: script.len(),
            cursor: 0,
            now: 0,
            sent: [None; SENT],
            sent_count: 0,
            deadlines: 0,
            may_send: |_| true,
            reloads: Reloads {
                default_reload: 50,
                enhanced_reload: 5_000,
            },
            asked: 0,
            refused: None,
            stalled: None,
            send_cost: 0,
            closes: 0,
        }
    }

    /// The same, the clock moving `ms` while each transmission is handed over.
    pub fn costing(mut self, ms: u32) -> Self {
        self.send_cost = ms;
        self
    }

    /// The same, refusing the `i`th transmission asked for (from zero) with `Err`.
    pub fn refusing(mut self, i: usize) -> Self {
        self.refused = Some(i);
        self
    }

    /// The same, pending once on the `i`th transmission asked for (from zero), after
    /// recording it.
    pub fn stalling(mut self, i: usize) -> Self {
        self.stalled = Some(i);
        self
    }

    /// The same, with the clock starting at `now`.
    pub fn starting_at(mut self, now: u32) -> Self {
        self.now = now;
        self
    }

    /// The same, failing the test on a transmission `may_send` refuses.
    pub fn sending_only(mut self, may_send: fn(Ai) -> bool) -> Self {
        self.may_send = may_send;
        self
    }

    /// The same, reporting `reloads` as its channel timing.
    pub fn with_reloads(mut self, reloads: Reloads) -> Self {
        self.reloads = reloads;
        self
    }

    /// Transmission `i`, if there was one.
    pub fn sent(&self, i: usize) -> Option<&Sent> {
        self.sent.get(i).and_then(Option::as_ref)
    }

    /// Transmission `i`'s bytes, or none.
    pub fn sent_bytes(&self, i: usize) -> &[u8] {
        self.sent(i).map_or(&[], Sent::bytes)
    }

    /// Transmission `i`'s addressing and bytes.
    pub fn addressed(&self, i: usize) -> (Option<Ai>, &[u8]) {
        (self.sent(i).map(|s| s.ai), self.sent_bytes(i))
    }

    /// Whether every step was taken.
    pub fn finished(&self) -> bool {
        self.cursor == self.len
    }

    fn record(&mut self, ai: Ai, data: &[u8], then: AfterSend) {
        let index = self.sent_count;
        assert!(
            (self.may_send)(ai),
            "transmission {index} ({data:02X?}) to {ai:?}"
        );
        let Some(slot) = self.sent.get_mut(index) else {
            panic!("transmission {index} ({data:02X?}) exceeds the {SENT} recorded");
        };
        let mut bytes = [0; FRAME];
        let Some(head) = bytes.get_mut(..data.len()) else {
            panic!("transmission {index} ({data:02X?}) exceeds {FRAME} bytes");
        };
        head.copy_from_slice(data);
        *slot = Some(Sent {
            ai,
            bytes,
            len: data.len(),
            at: self.now,
            after: self.cursor,
            then,
        });
        self.sent_count = self.sent_count.wrapping_add(1);
    }

    /// The next step, or the one error this transport returns: the script is exhausted.
    fn advance<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, ()> {
        let ind = |buffer: &'b mut [u8], ai, bytes: &[u8]| {
            let Some((head, _)) = buffer.split_at_mut_checked(bytes.len()) else {
                panic!("indication {bytes:02X?} does not fit the driver's buffer");
            };
            head.copy_from_slice(bytes);
            Ok(TransportEvent::DataInd { ai, data: head })
        };
        loop {
            let step = self.steps.get(self.cursor).copied().flatten().ok_or(())?;
            self.cursor = self.cursor.wrapping_add(1);
            match step {
                Step::Ind(ai, bytes) => return ind(buffer, ai, bytes),
                Step::IndAt(t, ai, bytes) => {
                    self.now = t;
                    return ind(buffer, ai, bytes);
                }
                Step::TooLong(ai, bytes) => {
                    let fit = buffer.len().min(bytes.len());
                    let (head, _) = buffer.split_at_mut(fit);
                    head.copy_from_slice(bytes.get(..fit).ok_or(())?);
                    return Ok(TransportEvent::DataTooLong {
                        ai,
                        data: head,
                        declared: Some(bytes.len()),
                    });
                }
                Step::Close(peer) => {
                    return Ok(TransportEvent::Closed {
                        peer,
                        expected: false,
                    });
                }
                Step::Leave(peer) => {
                    return Ok(TransportEvent::Closed {
                        peer,
                        expected: true,
                    });
                }
                Step::Conf(ai, result) => {
                    return Ok(TransportEvent::DataConf { ai, result });
                }
                Step::Slip(t) => self.now = t,
                Step::Pend => {}
                Step::At(t) => {
                    self.now = t;
                    // The comparison the seam doc prescribes, right across the wrap.
                    if deadline.is_some_and(|d| Timestamp(t).has_reached(d)) {
                        self.deadlines = self.deadlines.wrapping_add(1);
                        return Ok(TransportEvent::Deadline);
                    }
                }
            }
        }
    }
}

// Neither method is an `async fn`: one with no `.await` is
// `clippy::unused_async_trait_impl`, and one returning an `async` block is
// `clippy::manual_async_fn`. `next_event` must still be lazy: a driver creates a
// `next_event` future and drops it unpolled whenever something else wins its race, and a
// future that took its script step on creation would lose that step. `t_data_req`'s
// future is always awaited at once, so it takes its effect on creation.
impl UdsTransport for Script {
    type Error = ();
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        after: AfterSend,
    ) -> impl Future<Output = Result<(), ()>> {
        let asked = self.asked;
        self.asked = asked.wrapping_add(1);
        let result = if self.refused == Some(asked) {
            Err(())
        } else {
            self.record(ai, data, after);
            self.now = self.now.wrapping_add(self.send_cost);
            Ok(())
        };
        let mut stall = self.stalled == Some(asked);
        poll_fn(move |cx| {
            if core::mem::take(&mut stall) {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(result)
        })
    }
    fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> impl Future<Output = Result<TransportEvent<'b>, ()>> {
        let mut parts = Some((self, buffer));
        poll_fn(move |cx| {
            if let Some((t, _)) = parts.as_mut()
                && matches!(t.steps.get(t.cursor).copied().flatten(), Some(Step::Pend))
            {
                t.cursor = t.cursor.wrapping_add(1);
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(
                parts
                    .take()
                    .ok_or(())
                    .and_then(|(t, b)| t.advance(b, deadline)),
            )
        })
    }
    fn outbound_max(&self) -> Option<usize> {
        None
    }
    fn channel_timing(&self) -> Reloads {
        self.reloads
    }
    fn now(&self) -> Timestamp {
        Timestamp(self.now)
    }
}

impl ClientTransport for Script {
    fn close(&mut self) -> impl Future<Output = Result<(), ()>> {
        self.closes = self.closes.wrapping_add(1);
        core::future::ready(Ok(()))
    }
}

/// Pends `self.0` times before completing, waking itself each time.
#[derive(Debug)]
pub struct PendN(pub u8);

impl Future for PendN {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<()> {
        if self.0 == 0 {
            return Poll::Ready(());
        }
        self.0 = self.0.saturating_sub(1);
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Poll to completion with a no-op waker; every future here is ready within a bounded
/// number of polls.
pub fn block_on<F: Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    for _ in 0..64 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
    panic!("future did not complete in 64 polls");
}
