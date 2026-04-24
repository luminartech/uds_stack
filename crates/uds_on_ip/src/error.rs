//! Error types for UDS on IP operations.

use thiserror::Error;

/// Errors that can occur during UDS on IP operations.
#[derive(Debug, Error)]
pub enum Error {
    /// DoIP transport error.
    #[error("DoIP transport error: {0}")]
    Transport(#[from] simple_doip::Error),

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
