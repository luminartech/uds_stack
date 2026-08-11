//! Error types for UDS on IP operations.

use thiserror::Error;

/// Errors that can occur during UDS on IP operations.
#[derive(Debug, Error)]
pub enum Error {
    /// DoIP transport error.
    ///
    /// Note: this variant is populated via the manual
    /// [`From<simple_doip::Error>`] impl below rather than a `#[from]` derive,
    /// so that a routing-activation denial can be lifted into the dedicated,
    /// non-retryable [`Error::RoutingActivationDenied`] instead of being
    /// buried here.
    #[error("DoIP transport error: {0}")]
    Transport(simple_doip::Error),

    /// The `DoIP` entity refused routing activation. This is distinct from
    /// [`Error::Transport`] because it is *not* transient: retrying the
    /// connection cannot resolve it (the most common cause is that another
    /// tester — e.g. [PRODUCT_NAME_REDACTED] — already holds this tester's source address
    /// `0x0E00`). The reconnect machinery must therefore propagate it rather
    /// than loop, and the client's connection-error classifier returns `false`
    /// for the underlying `simple_doip` error.
    #[error(
        "Routing activation denied by the DoIP entity: {0:?} (another tester may already hold source address 0x0E00)"
    )]
    RoutingActivationDenied(simple_doip::messages::RoutingActivationResponseCode),

    /// Session timeout - no response received within the expected time.
    #[error("Session timeout: no response within {0:?}")]
    Timeout(std::time::Duration),

    /// Session not active - must activate routing before sending diagnostic messages.
    #[error("Session not active: routing activation required")]
    SessionNotActive,

    /// Connection closed unexpectedly.
    #[error("Connection closed")]
    ConnectionClosed,

    /// Invalid response received.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// UDS negative response received.
    #[error("UDS negative response: service {service_id:#04x}, NRC {nrc:#04x}")]
    NegativeResponse {
        /// The service ID that was rejected.
        service_id: u8,
        /// The Negative Response Code.
        nrc: u8,
    },

    /// Reconnection failed after multiple attempts.
    #[error("Reconnection failed after {attempts} attempts over {elapsed:?}")]
    ReconnectionFailed {
        /// Number of reconnection attempts made.
        attempts: u32,
        /// Total time spent trying to reconnect.
        elapsed: std::time::Duration,
    },

    /// Connection was lost but successfully restored.
    /// The original request may have succeeded (e.g., EcuReset causing reboot),
    /// but no response was received. The caller should proceed with subsequent requests.
    #[error("Connection lost and restored - no response received for request")]
    ReconnectedWithoutResponse,

    /// The server sent more consecutive NRC 0x78 (Response Pending) responses
    /// than `SessionConfig::max_response_pending_count` allows, suggesting it
    /// is stuck and will never produce a final response.
    #[error("server sent more than {max} consecutive NRC 0x78 responses without final answer")]
    Nrc78PendingExceeded {
        /// The configured maximum that was exceeded.
        max: u32,
    },
}

impl From<simple_doip::Error> for Error {
    /// Lift a `simple_doip` error into a `uds_on_ip` error.
    ///
    /// A routing-activation denial is promoted to the dedicated,
    /// non-retryable [`Error::RoutingActivationDenied`] so callers can
    /// distinguish "another tester owns this sensor" from an ordinary,
    /// retryable transport fault. Every other error is wrapped in
    /// [`Error::Transport`], matching the previous `#[from]` behavior.
    fn from(error: simple_doip::Error) -> Self {
        match error {
            simple_doip::Error::RoutingActivationDenied(code) => {
                Error::RoutingActivationDenied(code)
            }
            other => Error::Transport(other),
        }
    }
}
