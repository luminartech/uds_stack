//! Error taxonomy.
//!
//! [`Error`] is a concrete enum, and a short one: this crate is a transport,
//! so most of what can go wrong here reaches the driver as a *value* on the
//! event seam rather than as an error. An exchange's outcome is
//! `uds_services::TransportEvent::DataConf`'s `SResult`, a message too large
//! for the driver's buffer is `TransportEvent::DataTooLong`, and a connection
//! that went away is `TransportEvent::Closed`. Only a failure that leaves
//! nothing to report at all is an [`Error`].
//!
//! Two variants, therefore, and one absence worth stating. A *closed
//! connection* has no variant, and now needs none: ISO 14229-5:2022 REQ 7.9 and
//! REQ 7.11 make a server-initiated close part of the
//! `DiagnosticSessionControl` and `ECUReset` flows, so an expected close
//! arriving as an `Err` would have a driver treat a conformant flow as a
//! failure. `uds_services` published `TransportEvent::Closed { expected }` on
//! 2026-09-17, which is where it belongs.
//!
//! This type is [`uds_services::UdsTransport::Error`], so a socket failure is
//! this crate's to name. It has no variant yet — see
//! [`UdsTransport::next_event`](uds_services::UdsTransport::next_event) for
//! why there is no bound to take one from.

/// Errors raised by this crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A wire-level encode/decode failure: a `DoIP` message could not be
    /// encoded or decoded.
    ///
    /// This does not carry `simple_doip::Error`, that crate's async driver's
    /// error type, gated on its `client`/`server` features. This crate has no
    /// driver — it is a transport whose socket is the caller's — so the
    /// wire-level error is the one that crosses this boundary instead.
    #[error("DoIP message error: {0}")]
    Wire(#[from] simple_doip::messages::MessageError),

    /// The addressing cannot be carried over `DoIP`.
    #[error(transparent)]
    Mapping(#[from] crate::mapping::MappingError),
}

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
