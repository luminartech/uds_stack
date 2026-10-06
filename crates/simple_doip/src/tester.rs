//! The tester (client) role of the `DoIP` connection service, over `edge-nal`.

use crate::LogicalAddress;
use crate::messages::{NackCode, RoutingActivationResponseCode};
use crate::service::NotATesterAddress;

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the tester in the next commit")
)]
mod confirm;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the tester in the next commit")
)]
mod rx;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the tester in the next commit")
)]
mod tx;

/// Why a tester could not connect, or could not do what it was asked.
///
/// `E` is the socket's error, `edge_nal::TcpConnect::Error`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E: core::fmt::Debug> {
    /// The socket failed, connecting or carrying data. The connection is closed.
    #[error("socket failed: {0:?}")]
    Io(E),
    /// The source address the tester was given is outside the client range.
    #[error(transparent)]
    NotATesterAddress(#[from] NotATesterAddress),
    /// The entity refused routing activation with this response code
    /// (ISO 13400-2:2019 Table 49). The tester has closed the connection.
    ///
    /// Not retried: the only code the tester retries itself is
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`].
    ///
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`]:
    ///     RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired
    #[error("routing activation denied: {0:?}")]
    RoutingActivationDenied(RoutingActivationResponseCode),
    /// The entity answered routing activation for a tester address other than this
    /// one's. The tester has closed the connection.
    #[error("routing activation answered for tester {0}")]
    ActivationAnsweredForAnotherTester(LogicalAddress),
    /// The entity rejected the routing activation request's header
    /// (ISO 13400-2:2019 Table 19). The tester has closed the connection.
    #[error("routing activation request rejected: generic header NACK {0:?}")]
    HeaderNack(NackCode),
    /// The entity closed the connection before answering routing activation.
    #[error("connection closed during routing activation")]
    ClosedDuringActivation,
    /// A request is still awaiting its [`ConnectionEvent::Confirm`]; this one was not
    /// accepted.
    ///
    /// [`ConnectionEvent::Confirm`]: crate::service::ConnectionEvent::Confirm
    #[error("a request is still awaiting its confirm")]
    RequestPending,
    /// The PDU does not fit the tester's transmit queue; it was not accepted.
    #[error("the PDU does not fit the tester's buffer")]
    MessageTooLarge,
    /// The connection is closed and [`ConnectionEvent::Closed`] has been reported;
    /// reconnect to continue.
    ///
    /// [`ConnectionEvent::Closed`]: crate::service::ConnectionEvent::Closed
    #[error("not connected")]
    NotConnected,
}
