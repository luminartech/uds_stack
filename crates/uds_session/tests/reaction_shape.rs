//! Compile-time checks on the reaction drain's shape.
//!
//! These functions are never called. Type-checking them is the test: if the signatures
//! stop fitting together, the crate stops building.

use uds_session::{Reaction, Rejection};

/// ``UDSS_LLR_0011`` — outputs are drained by the caller, never pushed.
/// ``UDSS_LLR_0081`` — `finish` consumes the drain, so a rejection report cannot be
/// read before the expiry indications that must precede it.
///
/// The output type is a placeholder here: this task owns `Reaction`'s shape, and the
/// role output types arrive with their roles. `tests/server_surface.rs` exercises the
/// same drain over `ServerOutput`.
#[allow(dead_code, reason = "type-checked, never run; see the module comment")]
#[allow(
    clippy::while_let_on_iterator,
    reason = "spells out `Reaction::next` explicitly; never executed"
)]
fn a_reaction_is_drained_then_finished(mut r: Reaction<'_, '_, u8>) -> Result<(), Rejection> {
    while let Some(_output) = r.next() {}
    r.finish()
}

#[test]
fn the_shape_compiles() {
    // The assertion is the build itself.
}
