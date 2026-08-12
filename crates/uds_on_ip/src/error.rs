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
    /// tester — e.g. [PRODUCT_NAME_REDACTED] — already holds this tester's source address).
    /// The reconnect machinery must therefore propagate it rather than loop,
    /// and the client's connection-error classifier returns `false` for the
    /// underlying `simple_doip` error.
    ///
    /// The cause text is derived per code by [`routing_denial_cause`] — ISO
    /// 13400-2 defines eight denial codes with materially different meanings.
    #[error(
        "Routing activation denied by the DoIP entity ({:#04x}): {}",
        u8::from(*.0),
        routing_denial_cause(u8::from(*.0))
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

/// Explain a `DoIP` routing-activation denial in operator terms.
///
/// ISO 13400-2 defines eight denial codes whose causes are materially
/// different — and only two of them (`0x00`, `0x03`) implicate the tester's
/// own source address at all. Naming one cause for all eight sends an
/// operator chasing a tester conflict when the entity actually wanted
/// authentication or TLS, so every layer that renders a denial routes its
/// text through here.
///
/// Takes the raw byte rather than
/// [`RoutingActivationResponseCode`](simple_doip::messages::RoutingActivationResponseCode)
/// so consumers that store the code as a `u8` — to avoid a production
/// dependency on `simple_doip` — can share the same wording.
///
/// The returned phrase names the entity's reason only; it deliberately does
/// not name a concrete tester address, since that is the caller's
/// configuration (`UdsClientOptions::client_logical_address`), not this
/// layer's to assume.
#[must_use]
pub fn routing_denial_cause(code: u8) -> &'static str {
    match code {
        0x00 => "the entity does not recognize this tester's source address",
        0x01 => "the entity has no free diagnostic sockets",
        0x02 => "a different source address is already activated on this socket",
        0x03 => "another tester already holds this tester's source address",
        0x04 => "the entity requires authentication before routing activation",
        0x05 => "in-vehicle confirmation of the activation was rejected",
        0x06 => "the entity does not support the requested routing activation type",
        0x07 => "the entity requires a TLS-secured connection for this activation type",
        _ => "the entity returned a reserved or manufacturer-specific denial code",
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use simple_doip::messages::RoutingActivationResponseCode;

    /// The whole point of the per-code table is that an operator can tell the
    /// eight denials apart, so assert they are genuinely distinct rather than
    /// spot-checking one.
    #[test]
    fn every_denial_code_has_its_own_cause() {
        let causes: Vec<&str> = (0x00..=0x07).map(routing_denial_cause).collect();
        let mut unique = causes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            causes.len(),
            "denial codes 0x00-0x07 must not share wording: {causes:?}"
        );
    }

    /// The regression this table exists to prevent: `0x00` is the *opposite*
    /// diagnosis from `0x03`, and both used to render as "already registered".
    #[test]
    fn unknown_source_address_is_not_described_as_a_tester_conflict() {
        assert_eq!(
            routing_denial_cause(0x00),
            "the entity does not recognize this tester's source address"
        );
        assert_eq!(
            routing_denial_cause(0x03),
            "another tester already holds this tester's source address"
        );
    }

    #[test]
    fn reserved_codes_fall_through_to_the_catch_all() {
        assert_eq!(
            routing_denial_cause(0x42),
            "the entity returned a reserved or manufacturer-specific denial code"
        );
    }

    #[test]
    fn display_carries_both_the_raw_code_and_its_cause() {
        let err = Error::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedEncryptedConnectionViaTLSRequired,
        );
        let rendered = err.to_string();
        assert!(rendered.contains("0x07"), "raw code missing: {rendered}");
        assert!(
            rendered.contains("TLS-secured connection"),
            "per-code cause missing: {rendered}"
        );
    }
}
