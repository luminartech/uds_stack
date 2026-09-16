//! Error taxonomy.
//!
//! [`Error`] is a concrete enum. It was generic over the session layer's error
//! type while that was a trait's associated type; `uds_session` reports a
//! concrete `Rejection` now, so the parameter is gone and callers write
//! `Result<T>`.

use uds_session::{Rejection, SResult};

/// Errors raised by this crate.
///
/// No longer generic over a session-layer error: `uds_session` reports a
/// concrete [`Rejection`], and the trait whose associated type this used to
/// carry is deleted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A `DoIP` message could not be encoded or decoded.
    ///
    /// `simple_doip::Error` is its async driver's error type, gated on that
    /// crate's `client`/`server` features. This crate has no driver — it is a
    /// transport whose socket is the caller's — so the wire-level error is the
    /// one that crosses this boundary. A socket's own failures reach the caller
    /// through the transport surface, not through this variant.
    #[error("DoIP message error: {0}")]
    Wire(simple_doip::messages::MessageError),

    /// `tP_Client` expired without a complete response
    /// (ISO 14229-2:2021 REQ 5.16).
    ///
    /// For a functionally addressed request this is not an error: expiry is the
    /// defined signal that no further responses are coming
    /// (ISO 14229-5:2022 Figure 8), and the exchange ends normally instead.
    #[error("tP_Client expired with no complete response")]
    ResponseTimeout,

    /// The peer rejected the diagnostic message at the `DoIP` layer
    /// (payload `0x8003`).
    #[error("diagnostic message negatively acknowledged by the peer")]
    NegativeAcknowledgement,

    /// The connection closed and could not be re-established.
    ///
    /// Distinct from an *expected* close: ISO 14229-5:2022 REQ 7.9 and REQ 7.11
    /// make a server-initiated close part of the `DiagnosticSessionControl` and
    /// `ECUReset` flows, and those are handled rather than reported.
    #[error("connection closed")]
    ConnectionClosed,

    /// Re-establishing the connection after an expected close failed, so the
    /// service could not be completed (ISO 14229-5:2022 REQ 7.8, REQ 7.10).
    #[error("could not re-establish the connection and repeat routing activation")]
    ReactivationFailed,

    /// A periodic data record exceeded the non-segmented message limit
    /// (ISO 14229-5:2022 REQ 7.17).
    #[error("periodic data record of {len} bytes exceeds the non-segmented message limit")]
    PeriodicRecordTooLong {
        /// The offending record length.
        len: usize,
    },

    /// The caller's response buffer was too small to hold a response.
    ///
    /// An alloc-free API cannot grow a buffer on the caller's behalf, so the
    /// shortfall is reported instead.
    #[error("response buffer too small: needed {needed} bytes, had {available}")]
    BufferTooSmall {
        /// Bytes required.
        needed: usize,
        /// Bytes available.
        available: usize,
    },

    /// The session layer rejected the input.
    #[error("session layer rejected the input")]
    Session(Rejection),

    /// The exchange completed unsuccessfully.
    #[error("exchange did not complete: {0:?}")]
    Exchange(SResult),
}

impl From<simple_doip::messages::MessageError> for Error {
    fn from(e: simple_doip::messages::MessageError) -> Self {
        Self::Wire(e)
    }
}

/// This crate's result type.
pub type Result<T> = core::result::Result<T, Error>;

/// Compile-time proof that this crate's errors, and the upstream errors it
/// composes, implement [`core::error::Error`] rather than `std::error::Error`.
///
/// Since Rust 1.81 `std::error::Error` is a re-export of `core::error::Error`,
/// so a bound like this cannot distinguish them on a hosted target. It only
/// means something when compiled for a `*-none` target, where `std` does not
/// exist — which is exactly where the `no_std` build canary runs it.
const _: () = {
    const fn assert_core_error<T: core::error::Error>() {}
    assert_core_error::<simple_doip::messages::MessageError>();
    assert_core_error::<uds_protocol::Error>();
    assert_core_error::<Error>();
};
