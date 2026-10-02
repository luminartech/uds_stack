//! The drain every input returns.
//!
//! ``UDSS_LLR_0011`` requires outputs to be retrieved rather than pushed, and forbids the
//! session layer to invoke a callback, handler or caller-supplied trait implementation in
//! order to deliver one. ``UDSS_LLR_0013`` forbids retaining a payload past the input that
//! carried it and ``UDSS_LLR_0014`` has an output refer to the caller's own data, so an
//! output cannot outlive its input: a stored queue would have to hold a borrow whose
//! lifetime differs per call, which is unrepresentable without `unsafe`, and
//! `Cargo.toml` forbids `unsafe`.
//!
//! So an input returns this, the caller drains it through [`Reaction::outputs`], and then
//! consumes it to learn whether the input was accepted. ``UDSS_LLR_0081`` requires every
//! indication an expiry produced to precede both the input's own outputs and any
//! rejection report; consuming the drain to reach [`Reaction::finish`] makes that
//! ordering a property of the type rather than of the caller's discipline.

use crate::rejection::Rejection;
use crate::sealed::Sealed;
use core::marker::PhantomData;

/// What a session role yields when its expiry snapshots are drained.
///
/// ``UDSS_LLR_0081`` — every indication an expiry produced precedes the input's own
/// outputs, so a [`Reaction`] asks its session for these first. Sealed: the only
/// implementors are [`crate::Server`] and [`crate::Client`], which is what
/// ``UDSS_LLR_0011`` requires of a trait in a public bound.
pub trait Drain<'d, O>: Sealed {
    /// The next unreported expiry indication, taken from its slot; `None` when there is
    /// none left.
    fn next_expiry(&mut self) -> Option<O>;
}

/// The outputs of one input, and its outcome.
///
/// `'s` borrows the session `S`, `'d` the input's payload, `O` is the role's output type
/// and `T` what acceptance yields — [`crate::PhysicalChannelId`] for
/// `open_physical_channel`, [`crate::FunctionalChannelId`] for `open_functional_channel`,
/// `()` elsewhere. `S` is a type parameter rather than a trait object so that the
/// reaction keeps `Send`, the covariance of `'d`, and `Debug` for free.
///
/// Dropping a `Reaction` without draining it discards outputs the application needed,
/// which is why the type is `#[must_use]`. Expiry indications not drained are swept at
/// the session's next input.
///
/// # Draining
///
/// Drain through [`Reaction::outputs`], then consume with [`Reaction::finish`]:
///
/// ```
/// use uds_session::{
///     Association, Rejection, Server, ServerParams, ServerReaction, Timestamp,
/// };
///
/// fn handle(mut reaction: ServerReaction<'_, '_, 1>) -> Result<(), Rejection> {
///     for output in reaction.outputs() {
///         let _ = output; // handle each output, in order
///     }
///     reaction.finish()
/// }
///
/// let params = ServerParams {
///     s3_server: 5_000,
///     p2_server_max: 50,
///     p2_star_server_max: 5_000,
/// };
/// let mut server = Server::new([Association::EMPTY; 1], params);
/// assert!(handle(server.tick(Timestamp(0))).is_ok());
/// ```
///
/// [`Reaction::outputs`] borrows rather than consuming, so the reaction survives the loop
/// and [`Reaction::finish`] stays reachable afterwards. `Reaction` is deliberately not an
/// [`Iterator`] itself, so `for output in reaction` does not compile.
#[must_use = "an undrained reaction discards the outputs this input produced"]
#[derive(Debug)]
pub struct Reaction<'s, 'd, O, S: Drain<'d, O>, T = ()> {
    session: &'s mut S,
    own: [Option<O>; 2],
    outcome: Result<T, Rejection>,
    payload: PhantomData<&'d [u8]>,
}

impl<'s, 'd, O, S: Drain<'d, O>, T> Reaction<'s, 'd, O, S, T> {
    /// Built by the role's input methods, never by a caller.
    pub(crate) fn new(
        session: &'s mut S,
        own: [Option<O>; 2],
        outcome: Result<T, Rejection>,
    ) -> Self {
        Self {
            session,
            own,
            outcome,
            payload: PhantomData,
        }
    }

    /// Whether the input was accepted, and what acceptance yielded.
    ///
    /// ``UDSS_LLR_0016`` — the report states every cause that held. Consuming `self` is
    /// what enforces ``UDSS_LLR_0081``'s ordering.
    ///
    /// # Errors
    ///
    /// Returns the [`Rejection`] report if the input was rejected.
    pub fn finish(self) -> Result<T, Rejection> {
        self.outcome
    }

    /// The outputs this input produced, in the order ``UDSS_LLR_0081`` requires:
    /// expiry indications first, then the input's own.
    ///
    /// ``UDSS_LLR_0011`` — the caller retrieves them; nothing is pushed.
    pub fn outputs(&mut self) -> Outputs<'_, 'd, O, S> {
        Outputs {
            session: &mut *self.session,
            own: &mut self.own,
            payload: PhantomData,
        }
    }
}

/// The outputs of one input. Created by [`Reaction::outputs`].
#[derive(Debug)]
pub struct Outputs<'r, 'd, O, S: Drain<'d, O>> {
    session: &'r mut S,
    own: &'r mut [Option<O>; 2],
    payload: PhantomData<&'d [u8]>,
}

impl<'d, O, S: Drain<'d, O>> Iterator for Outputs<'_, 'd, O, S> {
    type Item = O;

    fn next(&mut self) -> Option<O> {
        self.session
            .next_expiry()
            .or_else(|| self.own.iter_mut().find_map(Option::take))
    }
}
