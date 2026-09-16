//! The ISO 14229-2 vocabulary is `uds_session`'s, and this crate does not
//! redefine it.
//!
//! These functions are never called. Type-checking them is the test.

#![allow(dead_code, reason = "type-checked, never run")]
#![allow(
    clippy::no_effect_underscore_binding,
    reason = "these bindings exist only to type-check, not to run"
)]

/// All four `Mtype` variants exist, including the two this crate's old
/// `addressing.rs` dropped because `DoIP` has no address extension.
fn all_four_message_types_are_expressible() {
    let _local = uds_session::Mtype::Diag;
    let _local_secure = uds_session::Mtype::SecureDiag;
    let _remote = uds_session::Mtype::RDiag {
        ae: uds_session::AddressExtension(0x0000),
    };
    let _remote_secure = uds_session::Mtype::SecureRDiag {
        ae: uds_session::AddressExtension(0xFFFF),
    };
}

/// The addressing triple is taken whole, not re-declared here.
fn the_addressing_triple_is_uds_sessions(ai: uds_session::Ai) -> uds_session::Address {
    ai.sa
}

#[test]
fn the_vocabulary_compiles() {
    // The assertion is the build itself.
}
