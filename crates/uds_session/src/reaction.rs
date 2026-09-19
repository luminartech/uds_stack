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
use core::marker::PhantomData;

/// The outputs of one input, and its outcome.
///
/// `'s` borrows the session, `'d` the input's payload, `O` is the role's output type and
/// `T` what acceptance yields — [`crate::PhysicalChannelId`] for `open_physical_channel`,
/// [`crate::FunctionalChannelId`] for `open_functional_channel`, `()` elsewhere.
///
/// Dropping a `Reaction` without draining it discards outputs the application needed,
/// which is why the type is `#[must_use]`.
///
/// # Draining
///
/// Drain through [`Reaction::outputs`], then consume:
///
/// ```
/// use uds_session::{Reaction, Rejection};
///
/// fn handle(mut reaction: Reaction<'_, '_, u8>) -> Result<(), Rejection> {
///     for output in reaction.outputs() {
///         let _ = output; // handle each output, in order
///     }
///     reaction.finish()
/// }
/// ```
///
/// [`Reaction::outputs`] borrows rather than consuming, so the reaction survives the loop
/// and [`Reaction::finish`] stays reachable afterwards. `Reaction` is deliberately not an
/// [`Iterator`] itself, so `for output in reaction` does not compile: it would move the
/// reaction into the loop and leave the outcome unreadable.
///
/// What the type does not enforce is that [`Reaction::finish`] is called at all — a
/// reaction that is drained and then dropped is accepted. ``UDSS_LLR_0081`` fixes the
/// order between the expiry indications and the report, so the outputs cannot be made to
/// arrive through the report instead, which is what enforcing the call would take.
#[must_use = "an undrained reaction discards the outputs this input produced"]
#[derive(Debug)]
pub struct Reaction<'s, 'd, O, T = ()> {
    outcome: Result<T, Rejection>,
    session: PhantomData<&'s mut ()>,
    payload: PhantomData<&'d [u8]>,
    output: PhantomData<fn() -> O>,
}

impl<O, T> Reaction<'_, '_, O, T> {
    /// Whether the input was accepted, and what acceptance yielded.
    ///
    /// ``UDSS_LLR_0016`` — the report states every cause that held and the content each
    /// rejecting requirement asked for. Consuming `self` is what enforces
    /// ``UDSS_LLR_0081``'s ordering.
    ///
    /// `finish` may be called without draining first, in which case the undrained outputs
    /// are discarded along with `self`. The type enforces only the *ordering*
    /// ``UDSS_LLR_0081`` requires between expiry indications and the report, not that
    /// every output is actually delivered to the caller.
    ///
    /// # Errors
    ///
    /// Returns the [`Rejection`] report if the input was rejected.
    #[allow(
        clippy::missing_const_for_fn,
        reason = "not const once the drain holds state"
    )]
    pub fn finish(self) -> Result<T, Rejection> {
        self.outcome
    }

    /// The outputs this input produced, in the order ``UDSS_LLR_0081`` requires.
    ///
    /// ``UDSS_LLR_0011`` — the caller retrieves them; nothing is pushed. The iterator
    /// borrows the reaction, so draining does not consume it and
    /// [`Reaction::finish`] remains reachable afterwards. See `# Draining`.
    pub fn outputs(&mut self) -> Outputs<'_, O> {
        Outputs {
            reaction: PhantomData,
            output: PhantomData,
        }
    }
}

/// The outputs of one input. Created by [`Reaction::outputs`].
///
/// ``UDSS_LLR_0081`` — every indication an expiry produced comes first, then the input's
/// own outputs. Dropping this before it is exhausted discards the rest.
#[derive(Debug)]
pub struct Outputs<'r, O> {
    reaction: PhantomData<&'r mut ()>,
    output: PhantomData<fn() -> O>,
}

impl<O> Iterator for Outputs<'_, O> {
    type Item = O;

    fn next(&mut self) -> Option<O> {
        #[allow(
            clippy::todo,
            reason = "API stub; behaviour lands with its requirement"
        )]
        {
            todo!(
                "outputs are generated from session state; see UDSS_LLR_0081 for \
                 their order"
            )
        }
    }
}
