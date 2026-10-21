//! The ISO 14229-2 vocabulary is `uds_session`'s, and this crate does not
//! redefine it.
//!
//! A compile-time test: it passes by compiling, and these functions are never called.

/// All four `Mtype` variants exist, including the two `DoIP` cannot carry for want
/// of an address extension, which this crate rejects in `mapping` rather than removes.
#[expect(dead_code, reason = "compile-time test: compiled, never called")]
#[expect(
    clippy::no_effect_underscore_binding,
    reason = "compile-time test: the bindings are type-checked, never used"
)]
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
#[expect(dead_code, reason = "compile-time test: compiled, never called")]
fn the_addressing_triple_is_uds_sessions(ai: uds_session::Ai) -> uds_session::Address {
    ai.sa
}

#[test]
fn the_vocabulary_compiles() {
    // Empty: the test is that this file compiles.
}
