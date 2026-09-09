//! Error taxonomy.
//!
//! [`Error`] is generic over the session layer's error type rather than
//! erasing it into a boxed or stringified form, because boxing needs `alloc`
//! and this crate's core is alloc-free. The generic is threaded through
//! [`Result`], so callers normally write `Result<T, S::Error>` and never name
//! it directly.

use crate::session::AResult;

/// Errors raised by this crate.
///
/// `E` is the session layer's own error type
/// ([`SessionLayer::Error`](crate::session::SessionLayer::Error)).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E> {
    /// The `DoIP` transport failed.
    ///
    /// Only present with a driver feature enabled: `simple_doip::Error` is the
    /// async driver's error type and does not exist in a `no_std` build. That
    /// gating is correct rather than incidental — a transport failure is a
    /// property of the driver, and the alloc-free core performs no I/O to fail
    /// at.
    #[cfg(any(feature = "client", feature = "server"))]
    #[error("DoIP transport error: {0}")]
    Transport(simple_doip::Error),

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

    /// The session layer rejected the request.
    #[error("session layer error")]
    Session(E),

    /// The exchange completed unsuccessfully.
    #[error("exchange did not complete: {0:?}")]
    Exchange(AResult),
}

#[cfg(any(feature = "client", feature = "server"))]
impl<E> From<simple_doip::Error> for Error<E> {
    fn from(e: simple_doip::Error) -> Self {
        Self::Transport(e)
    }
}

/// This crate's result type, generic over the session layer's error.
pub type Result<T, E> = core::result::Result<T, Error<E>>;

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
    assert_core_error::<Error<core::convert::Infallible>>();
};
