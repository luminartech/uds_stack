//! Error taxonomy.
//!
//! [`Error`] is a concrete enum. It was generic over the session layer's error
//! type while that was a trait's associated type; that parameter is gone, and
//! callers write `Result<T>`.

use uds_session::SResult;

/// Errors raised by this crate.
///
/// No longer generic over a session-layer error: the trait whose associated
/// type this used to carry is deleted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A wire-level encode/decode failure: a `DoIP` message could not be
    /// encoded or decoded.
    ///
    /// This does not carry `simple_doip::Error`, that crate's async driver's
    /// error type, gated on its `client`/`server` features. This crate has no
    /// driver — it is a transport whose socket is the caller's — so the
    /// wire-level error is the one that crosses this boundary instead. A
    /// socket's own failures reach the caller through the transport surface,
    /// not through this variant.
    #[error("DoIP message error: {0}")]
    Wire(#[from] simple_doip::messages::MessageError),

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

    /// The exchange completed unsuccessfully.
    #[error("exchange did not complete: {0:?}")]
    Exchange(SResult),

    /// The addressing cannot be carried over `DoIP`.
    #[error(transparent)]
    Mapping(#[from] crate::mapping::MappingError),
}

/// This crate's result type.
pub type Result<T> = core::result::Result<T, Error>;

/// Compile-time proof that this crate's errors, and the upstream errors it
/// composes, implement [`core::error::Error`] rather than `std::error::Error`.
///
/// Since Rust 1.81 `std::error::Error` is a re-export of `core::error::Error`,
/// so a bound like this cannot distinguish them on a hosted target. It only
/// means something when compiled for a `*-none` target, where `std` does not
/// exist — which is where a `*-none` target build would exercise it.
const _: () = {
    const fn assert_core_error<T: core::error::Error>() {}
    assert_core_error::<simple_doip::messages::MessageError>();
    assert_core_error::<uds_protocol::Error>();
    assert_core_error::<Error>();
};
