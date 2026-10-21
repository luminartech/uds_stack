//! The drain every input returns.
//!
//! ``UDSS_LLR_0011`` requires outputs to be retrieved rather than pushed: an input returns
//! a [`Reaction`], which the caller drains through [`Reaction::outputs`] and consumes with
//! [`Reaction::finish`] to learn whether the input was accepted. ``UDSS_LLR_0013`` and
//! ``UDSS_LLR_0014`` have an output borrow the input's own payload, so the input's outputs
//! cannot outlive the reaction; an expiry's indication borrows nothing and stays in the
//! session until it is retrieved. ``UDSS_LLR_0081`` orders expiry indications before the
//! input's own outputs and before any rejection report, and the drain yields them so.

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
/// and `T` what acceptance yields: [`crate::PhysicalChannelId`] for
/// `open_physical_channel`, [`crate::FunctionalChannelId`] for `open_functional_channel`,
/// `()` elsewhere. `S` is a type parameter rather than a trait object so that the
/// reaction keeps `Send`, the covariance of `'d`, and `Debug`.
///
/// The input is applied before the reaction is returned; only reporting it is left to
/// the drain, so the session's state is final whatever the caller does with the reaction.
/// Dropping a `Reaction` without draining it discards the input's own outputs, which is
/// why the type is `#[must_use]`; expiry indications it was not drained of stay in the
/// session and come first in the next input's drain.
///
/// # Draining
///
/// Drain through [`Reaction::outputs`], then consume with [`Reaction::finish`]:
///
/// ```
/// use uds_session::{
///     Association, Finished, Rejection, Server, ServerParams, ServerReaction, Timestamp,
/// };
///
/// fn handle(mut reaction: ServerReaction<'_, '_, 1>) -> Result<(), Rejection> {
///     for output in reaction.outputs() {
///         let _ = output; // handle each output, in order
///     }
///     let Finished { outcome, rest } = reaction.finish();
///     assert_eq!(rest.count(), 0, "the loop above drained everything");
///     outcome
/// }
///
/// let params = ServerParams::new(5_000, 50, 5_000, 0);
/// let mut server = Server::new([Association::EMPTY; 1], params);
/// assert!(handle(server.tick(Timestamp(0))).is_ok());
/// ```
///
/// [`Reaction::outputs`] borrows rather than consuming, so the reaction survives the loop
/// and [`Reaction::finish`] stays reachable afterwards. `Reaction` is not an [`Iterator`]
/// itself, so `for output in reaction` does not compile.
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

    /// Whether the input was accepted, with every output not yet drained.
    ///
    /// ``UDSS_LLR_0016`` — the outcome's report states every cause that held.
    /// ``UDSS_LLR_0011`` — what [`Reaction::outputs`] did not yield is in
    /// [`Finished::rest`], in the order ``UDSS_LLR_0081`` requires; after a full drain it
    /// yields nothing.
    pub fn finish(self) -> Finished<'s, 'd, O, S, T> {
        Finished {
            outcome: self.outcome,
            rest: Rest {
                session: self.session,
                own: self.own,
                payload: PhantomData,
            },
        }
    }

    /// The outputs this input produced, in the order ``UDSS_LLR_0081`` requires:
    /// expiry indications first, then the input's own.
    ///
    /// Among expiries that fell due together the requirement fixes no order, and this
    /// crate guarantees one, so a change to it is a breaking change. A server reports
    /// `tS3_Server`'s before `tP2_Server`'s. A client reports its physical channels in
    /// slot order, each one's response timeout before its keep-alive, then its
    /// functional channels in slot order, then the client-wide keep-alive.
    ///
    /// ``UDSS_LLR_0011`` — the caller retrieves them; nothing is pushed. A drain that is
    /// not iterated leaves them for [`Reaction::finish`].
    #[must_use = "the session's outputs are lost unless the drain is iterated"]
    pub fn outputs(&mut self) -> Outputs<'_, 'd, O, S> {
        Outputs {
            session: &mut *self.session,
            own: &mut self.own,
            payload: PhantomData,
        }
    }
}

/// A finished [`Reaction`]: its outcome, and the outputs the caller had not drained.
#[must_use = "the outputs in `rest` are lost unless it is iterated"]
#[derive(Debug)]
pub struct Finished<'s, 'd, O, S: Drain<'d, O>, T = ()> {
    /// Whether the input was accepted, and what acceptance yielded.
    ///
    /// # Errors
    ///
    /// The [`Rejection`] report, if the input was rejected.
    pub outcome: Result<T, Rejection>,
    /// The outputs not yet drained, expiry indications first. Empty after a full drain.
    pub rest: Rest<'s, 'd, O, S>,
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
        next_output(self.session, self.own)
    }
}

/// The outputs a finished reaction had not yielded. Found in [`Finished::rest`].
///
/// An expiry indication left in it when it is dropped stays in the session for the next
/// input's drain; an output of the input itself is gone.
#[must_use = "the outputs of the input are lost unless this is iterated"]
#[derive(Debug)]
pub struct Rest<'s, 'd, O, S: Drain<'d, O>> {
    session: &'s mut S,
    own: [Option<O>; 2],
    payload: PhantomData<&'d [u8]>,
}

impl<'d, O, S: Drain<'d, O>> Iterator for Rest<'_, 'd, O, S> {
    type Item = O;

    fn next(&mut self) -> Option<O> {
        next_output(self.session, &mut self.own)
    }
}

fn next_output<'d, O, S: Drain<'d, O>>(
    session: &mut S,
    own: &mut [Option<O>; 2],
) -> Option<O> {
    session
        .next_expiry()
        .or_else(|| own.iter_mut().find_map(Option::take))
}
