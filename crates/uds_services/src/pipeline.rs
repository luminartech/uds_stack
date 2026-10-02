//! The clause 8.7 pipeline. ``UDSSVC_ARCH_0004`` — an ordered sequence of stages, each
//! settling the request or passing it on; ``UDSSVC_ARCH_0018`` — nothing outside this
//! crate calls it. Every item is `pub` only because `uds_server!` expands in the
//! application's crate and routes here; none is API an application writes against.

use crate::services::SessionTransition;
use crate::state::State;
use crate::{Responded, ResponseSink};
use uds_protocol::DiagnosticSessionType;

/// Enter `to`, and say which of Figure 7's transitions that was (``UDSSVC_ARCH_0038``).
/// The one place the session field is written; the macro's hooks call this and nothing
/// else, so `State`'s accessors stay crate-private and no clause 10.2 logic is emitted
/// into the application's crate.
#[doc(hidden)]
pub fn transition(state: &mut State, to: DiagnosticSessionType) -> SessionTransition {
    let from = state.session();
    state.set_session(to);
    SessionTransition::classify(from, to)
}

/// Placeholder until Task 12 routes `dispatch` here. ``UDSSVC_ARCH_0004``.
///
/// Not `async`, as the pipeline it stands in for is: it awaits nothing, and
/// `clippy::unused_async` refuses an `async fn` that does not.
#[doc(hidden)]
#[must_use]
pub const fn dispatch_stub(_request: &[u8], _out: &mut ResponseSink<'_>) -> Responded {
    Responded::Suppressed { session: None }
}
