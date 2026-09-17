//! Error taxonomy.
//!
//! [`Error`] is a concrete enum, and a short one: this crate is a transport,
//! so most of what can go wrong here reaches the caller as a *value* on the
//! event seam rather than as an error. An exchange's outcome is
//! [`TransportEvent::DataConf`](crate::TransportEvent)'s `SResult`, a message
//! too large for the caller's buffer is
//! [`TransportEvent::DataTooLong`](crate::TransportEvent), and what to do with
//! the connection after an exchange is [`profile::PostExchange`](crate::profile::PostExchange).
//! Only a failure that leaves nothing to report at all is an [`Error`].

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

    /// The connection closed.
    ///
    /// # This variant is provisional
    ///
    /// ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a server-initiated close part
    /// of the `DiagnosticSessionControl` and `ECUReset` flows, so a close is
    /// routinely *expected* rather than a fault — and this crate cannot act on
    /// either kind. It does not reconnect: [`profile::post_exchange`] returns
    /// [`ReconnectAndReactivate`] as advice the caller acts on.
    ///
    /// So an expected close and an unexpected one both end up here, which is
    /// the wrong shape, and the right one is a case on the event seam that
    /// `uds_services` owns. Raised with them 2026-09-17; this variant holds
    /// the fact until that lands.
    ///
    /// [`profile::post_exchange`]: crate::profile::post_exchange
    /// [`ReconnectAndReactivate`]: crate::profile::PostExchange::ReconnectAndReactivate
    #[error("connection closed")]
    ConnectionClosed,

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
