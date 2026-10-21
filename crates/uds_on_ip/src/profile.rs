//! Application layer: the `UDSonIP` profile.
//!
//! ISO 14229-5:2022 clause 8, REQ 7.1–7.20: how a UDS message is framed into a `DoIP`
//! diagnostic message, and what the transport does around particular services.
//!
//! The `tP_Client` reload pair is [`uds_session::Reloads`], not a type of this crate's.
//! What this crate contributes is the choice of parameter: `DoIP` offers no
//! `T_DataSOM.ind`, so ISO 14229-2:2021 REQ 5.11 gives it the `tP6` pair rather than
//! the `tP2` pair, and the session layer's field names say neither.

use uds_services::AfterSend;

/// Conventional bench reload values, for a desk setup and for tests: `tP6_Client_Max`
/// 2000 ms and `tP6*_Client_Max` 5000 ms, in `uds_session::Timestamp`'s unit.
///
/// These are **not** a configuration for a vehicle. ISO 14229-2:2021 clause 9.2 Table 4
/// specifies the reloads as minima derived from the server's timing and the vehicle
/// network's delays, which this crate cannot know; on a live bus these values produce
/// spurious `tP6` timeouts. A named function rather than a `Default`, so bench values
/// are reached only by asking for them by name.
#[must_use]
pub const fn bench_reloads() -> uds_session::Reloads {
    uds_session::Reloads {
        default_reload: 2_000,
        enhanced_reload: 5_000,
    }
}

/// The service identifiers ISO 14229-5 clause 8 keys connection handling on.
///
/// `ARCHITECTURE.md` §10, invariant 1: `uds_on_ip` never learns what a service is. This
/// module, with [`crate::mapping`]'s `awaits_response` and `response_pending`, is the
/// exhaustive list of what it reads of a message:
///
/// - the positive `DiagnosticSessionControl` and `ECUReset` response identifiers,
///   after which REQ 7.9 and REQ 7.11 have the server close the connection;
/// - a request's suppressPosRspMsgIndicationBit, to know whether an answer is owed;
/// - the response-pending negative response, after which the final one is still owed.
///
/// No data identifier, routine identifier or NRC policy. The identifiers are taken
/// from `uds_protocol`'s [`UdsServiceType`](uds_protocol::UdsServiceType). What
/// reaches this crate is a `T_PDU`, whose first octet is the service identifier only
/// because REQ 4.4 Table 5 maps `T_Data` onto the `DoIP` diagnostic message's user data
/// unchanged and `DoIP` performs no segmentation; a binding for a segmented transport
/// re-derives it rather than copying this module.
pub(crate) mod service_ids {
    use uds_protocol::UdsServiceType;

    /// The positive response to `DiagnosticSessionControl` (ISO 14229-5:2022 REQ 7.9).
    pub(crate) const SESSION_CHANGED: u8 =
        UdsServiceType::DiagnosticSessionControl.to_response_sid();

    /// The positive response to `ECUReset` (ISO 14229-5:2022 REQ 7.11).
    pub(crate) const RESETTING: u8 = UdsServiceType::EcuReset.to_response_sid();
}

/// What ISO 14229-5 clause 8 requires of a server's connection once it has sent a
/// message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionAction {
    /// Nothing follows. The connection continues to serve the session.
    Continue,
    /// The server sent a positive `ECUReset` response (REQ 7.11), or the positive
    /// `DiagnosticSessionControl` response to a session change that leaves its running
    /// software (REQ 7.9), and **must itself initiate** the close, after the response
    /// and before executing the service.
    InitiateClose,
}

impl ConnectionAction {
    /// Whether a close arriving now is one the standard prescribes: the
    /// `uds_services::TransportEvent::Closed` `expected` flag.
    pub(crate) const fn close_is_prescribed(self) -> bool {
        matches!(self, Self::InitiateClose)
    }
}

/// Classify a message a server is about to send, by its first octet and by what the
/// server says follows it. REQ 7.11's close follows every positive `ECUReset` response,
/// so the octet decides it; REQ 7.9's close follows a session change only if it leaves
/// the software the server is running, which no octet carries, so the server states it
/// as [`AfterSend::ServerLeaves`] on the message the close follows.
///
/// A negative response, response-pending included, has first octet `0x7F` and is
/// [`Continue`](ConnectionAction::Continue); the final positive response triggers the
/// close. A suppressed `11 81` or `10 82` sends nothing, so nothing is classified: a
/// server that resets or leaves on one drops the connection unannounced
/// (`ARCHITECTURE.md` §6.2).
#[must_use]
pub(crate) const fn after_sending(first_octet: u8, after: AfterSend) -> ConnectionAction {
    match (first_octet, after) {
        (_, AfterSend::ServerLeaves) | (service_ids::RESETTING, _) => {
            ConnectionAction::InitiateClose
        }
        _ => ConnectionAction::Continue,
    }
}

/// Whether a client that has received `response` may see the server close the
/// connection next, as a prescribed close (REQ 7.9, REQ 7.11).
///
/// True for any positive `DiagnosticSessionControl` response, because whether the
/// session change leaves the server's software is the server's to know.
#[must_use]
pub(crate) fn server_may_close_after(response: &[u8]) -> bool {
    matches!(
        response.first(),
        Some(&(service_ids::SESSION_CHANGED | service_ids::RESETTING))
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionAction, after_sending, bench_reloads, server_may_close_after, service_ids,
    };
    use uds_services::AfterSend;

    /// The octets clause 8 keys connection handling on. Spelled out as literals rather
    /// than taken from `service_ids`, so the constants and the rules cannot drift
    /// together.
    #[test]
    fn clause_8_keys_on_two_response_octets() {
        assert_eq!(service_ids::SESSION_CHANGED, 0x50);
        assert_eq!(service_ids::RESETTING, 0x51);
    }

    /// REQ 7.11: every positive `ECUReset` response owes the server's close.
    #[test]
    fn a_positive_reset_response_owes_the_close() {
        assert_eq!(
            after_sending(0x51, AfterSend::Continue),
            ConnectionAction::InitiateClose
        );
    }

    /// ISO 14229-5:2022 REQ 7.9 closes after a positive `DiagnosticSessionControl`
    /// response only if the session change disconnects, which the octet cannot
    /// say: the server states it, so the octet alone owes nothing.
    #[test]
    fn a_positive_session_response_alone_owes_no_close() {
        assert_eq!(
            after_sending(0x50, AfterSend::Continue),
            ConnectionAction::Continue
        );
    }

    /// The server saying it leaves its running software owes REQ 7.9's close,
    /// whatever the octet: which message carries it is the server's to choose.
    #[test]
    fn a_server_leaving_owes_the_close() {
        assert_eq!(
            after_sending(0x50, AfterSend::ServerLeaves),
            ConnectionAction::InitiateClose
        );
    }

    /// Nothing else in the byte range owes a server anything: 255 of the 256 first
    /// octets are `Continue`. A negative response, `0x7F`, is among them, so a
    /// response-pending never ends an exchange the server is still working on.
    #[test]
    fn every_other_octet_is_continue_for_a_server() {
        for octet in (0x00..=0xFF_u8).filter(|&octet| octet != 0x51) {
            assert_eq!(
                after_sending(octet, AfterSend::Continue),
                ConnectionAction::Continue,
                "{octet:#04X} is not a message clause 8 keys a server's close on",
            );
        }
    }

    /// A client may see a prescribed close after either positive response, and after
    /// nothing else; a response-pending to either service is not one.
    #[test]
    fn a_client_expects_a_close_only_after_the_two_positive_responses() {
        for octet in 0x00..=0xFF_u8 {
            assert_eq!(
                server_may_close_after(&[octet, 0x01]),
                matches!(octet, 0x50 | 0x51),
                "{octet:#04X}",
            );
        }
        assert!(!server_may_close_after(&[0x7F, 0x11, 0x78]));
        assert!(!server_may_close_after(&[]));
    }

    /// Only a close the server owes reports `expected`.
    #[test]
    fn only_a_prescribed_flow_reports_expected() {
        assert!(ConnectionAction::InitiateClose.close_is_prescribed());
        assert!(
            !ConnectionAction::Continue.close_is_prescribed(),
            "a drop during an ordinary exchange is not prescribed by anything",
        );
    }

    /// The bench values are the `tP6` pair, and they are the ones a reader of
    /// ISO 14229-2:2021 clause 9.2 Table 4 would recognize as conventional.
    #[test]
    fn the_bench_reloads_are_the_conventional_pair() {
        let reloads = bench_reloads();
        assert_eq!(reloads.default_reload, 2_000);
        assert_eq!(reloads.enhanced_reload, 5_000);
        assert!(
            reloads.enhanced_reload > reloads.default_reload,
            "tP6* extends tP6; a pair that did not would stall a response-pending flow",
        );
    }
}
