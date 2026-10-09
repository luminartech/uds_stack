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
//! A *closed connection* has no variant: ISO 14229-5:2022 REQ 7.9 and REQ 7.11
//! make a server-initiated close part of the `DiagnosticSessionControl` and
//! `ECUReset` flows, so an expected close arriving as an `Err` would have a
//! driver treat a conformant flow as a failure.

/// Errors raised by [`DoIpTransport`](crate::DoIpTransport), whose entity fails
/// with `E`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E> {
    /// The `DoIP` entity failed as a whole: its
    /// [`DiagnosticEntity::Error`](simple_doip::service::DiagnosticEntity::Error).
    #[error("the DoIP entity failed: {0:?}")]
    Entity(E),

    /// The entity refused a request while the transport already held as many refused
    /// requests' failed confirmations as it can, none of them yet reported.
    #[error("the DoIP entity refused a request with no room left to confirm it: {0}")]
    Refused(simple_doip::service::Refusal),

    /// The addressing cannot be carried over `DoIP`.
    #[error(transparent)]
    Mapping(#[from] crate::mapping::MappingError),

    /// The entity reported a PDU outside the buffer it was lent, breaking
    /// [`DiagnosticEntity::next_event`](simple_doip::service::DiagnosticEntity::next_event)'s
    /// contract.
    #[error("the DoIP entity reported a PDU outside the buffer it was lent")]
    PduOutsideBuffer,
}

/// Errors raised by [`DoIpClientTransport`](crate::DoIpClientTransport) over a
/// [`TesterConnection`](simple_doip::service::TesterConnection) that fails with `E`,
/// reconnects failing with `R`, and closes failing with `X`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientTransportError<E, R, X> {
    /// The connection failed other than by closing: its
    /// [`DiagnosticConnection::Error`](simple_doip::service::DiagnosticConnection::Error).
    #[error("the DoIP connection failed: {0:?}")]
    Connection(E),

    /// No connection could be opened and activated for a request: the
    /// [`TesterConnection::ReconnectError`](simple_doip::service::TesterConnection::ReconnectError).
    /// The request was not accepted, and the connection stays closed until a later
    /// request's reconnect succeeds.
    #[error("the DoIP connection could not be reopened: {0:?}")]
    Reconnect(R),

    /// Closing the connection failed: the
    /// [`TesterConnection::CloseError`](simple_doip::service::TesterConnection::CloseError).
    /// The connection is closed all the same.
    #[error("the DoIP connection did not close cleanly: {0:?}")]
    Close(X),

    /// The connection refused a request while the transport already owed as many
    /// confirmations as it can hold, none of them yet reported.
    #[error("the DoIP connection refused a request with no room left to confirm it: {0}")]
    Refused(simple_doip::service::Refusal),

    /// The addressing cannot be carried over `DoIP`.
    #[error(transparent)]
    Mapping(#[from] crate::mapping::MappingError),

    /// The connection reported a PDU outside the buffer it was lent, breaking
    /// [`DiagnosticConnection::next_event`](simple_doip::service::DiagnosticConnection::next_event)'s
    /// contract.
    #[error("the DoIP connection reported a PDU outside the buffer it was lent")]
    PduOutsideBuffer,
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
    assert_core_error::<uds_protocol::Error>();
    assert_core_error::<Error<core::convert::Infallible>>();
    assert_core_error::<
        ClientTransportError<
            core::convert::Infallible,
            core::convert::Infallible,
            core::convert::Infallible,
        >,
    >();
};
