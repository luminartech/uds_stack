//! Application layer: the `UDSonIP` profile.
//!
//! ISO 14229-5:2022 clause 8, REQ 7.1–7.20. This is the clause 8 profile the
//! transport applies, and it is where the genuinely IP-specific behaviour
//! lives: how a UDS message is framed into a `DoIP` diagnostic message, and
//! what the transport must do around particular services.
//!
//! The `tP_Client` reload pair is [`uds_session::Reloads`] and is deliberately
//! not redeclared here. This crate's contribution is not the type but the
//! *choice of parameter*: `DoIP` offers no `T_DataSOM.ind`, so ISO 14229-2:2021
//! REQ 5.11 gives it the `tP6` pair rather than the `tP2` pair. The session
//! layer does not distinguish the two cases — which is why its field names do
//! not say `p6` — and the distinction survives in the values this transport
//! supplies, not in a second type.

use uds_services::AfterSend;

/// Conventional bench reload values, for a desk setup and for tests.
///
/// `tP6_Client_Max` 2000 ms and `tP6*_Client_Max` 5000 ms. These are a starting
/// point on a bench and are **not** a configuration for a vehicle:
/// ISO 14229-2:2021 clause 9.2 Table 4 specifies the reloads as minima derived
/// from the server's timing and the vehicle network's delays, which this crate
/// cannot know. Shipping these is how a live bus produces spurious `tP6`
/// timeouts that get blamed on the ECU.
///
/// A named function rather than a `Default`, so bench values cannot be reached
/// without asking for them by name — `Reloads` itself has no `Default`, and
/// this crate does not add the strongest invitation Rust has to the one value
/// nobody should accept.
///
/// # There is deliberately no `Spacing` here
///
/// `tP3_Client_Phys` and `tP3_Client_Func` were a `Spacing` struct on a `Timing`
/// type this module owned. Nothing read either field: `channel_timing` returns
/// the reloads alone, and `uds_services::UdsTransport::channel_timing` says why
/// that is right — a transport dictates the reload pair, because the pair
/// follows from having no `T_DataSOM.ind`, but has no view on spacing, which is
/// ISO 14229-2:2021 clause 9.7 client policy. Carrying a field this crate
/// stores and never applies taught a caller that configuring it did something.
///
/// Values are milliseconds, matching `uds_session::Timestamp`'s unit. Carrying a
/// `Duration` would mean converting at the seam on every call.
#[must_use]
pub const fn bench_reloads() -> uds_session::Reloads {
    uds_session::Reloads {
        default_reload: 2_000,
        enhanced_reload: 5_000,
    }
}

/// Service identifiers whose *transport* behaviour ISO 14229-5 specifies.
///
/// # Why this crate knows any service identifier at all
///
/// `ARCHITECTURE.md` §13, invariant 5, states that `uds_on_ip` never learns
/// what a service is. These constants are the exception the standard itself forces, and
/// the exhaustive list of it.
///
/// ISO 14229-5:2022 REQ 7.8–7.11 make TCP connection handling part of the
/// `DiagnosticSessionControl` and `ECUReset` flows specifically. REQ 7.11 is
/// unconditional: the server closes the connection after every positive
/// `ECUReset` response and before executing the reset. REQ 7.8 and REQ 7.10 have
/// the client establish a new connection *and repeat routing activation* when
/// one is closed. That is transport behaviour keyed on a service identifier,
/// and clause 8 puts it in the IP profile rather than in ISO 14229-1.
///
/// REQ 7.9's close after a positive `DiagnosticSessionControl` response is
/// *conditional* — "if the TCP connection is disconnected due to a session
/// change" — and what decides it is whether the server leaves the software it
/// is running, which no octet carries. The positive response's identifier is
/// therefore not here: the server states that close across the seam as
/// `uds_services::AfterSend::ServerLeaves`, and this crate does not infer it
/// (`ARCHITECTURE.md` §9).
///
/// So the invariant is narrower than first written: this crate knows these three
/// identifiers and nothing else. No sub-function, no data identifier, no
/// routine identifier, no NRC policy.
///
/// # The read is of a `T_PDU`, and that is confirmed rather than assumed
///
/// Clause 8 keys this handling on a service identifier, and under this
/// stack's design the driver sits above the session layer — so what reaches
/// this crate is a `T_PDU`, not an `A_PDU`.
///
/// On `DoIP` the distinction is nominal. ISO 14229-5:2022 REQ 4.4 Table 5
/// maps `T_Data` onto the `DoIP` diagnostic message's user data unchanged,
/// and `DoIP` performs no segmentation — the same fact ISO 14229-2:2021
/// REQ 5.11 rests on when it gives `DoIP` the `tP6` pair for having no
/// `T_DataSOM.ind`. The two PDUs are therefore the same octets, and the
/// first is still the service identifier.
///
/// On a segmented transport this would not hold, so a binding for one must
/// re-derive it rather than copy this module. That is a reading of the
/// standards rather than a property a test can hold.
pub(crate) mod service_ids {
    use uds_protocol::UdsServiceType;

    /// `DiagnosticSessionControl` (ISO 14229-5:2022 REQ 7.8, REQ 7.9).
    pub(crate) const DIAGNOSTIC_SESSION_CONTROL: u8 =
        UdsServiceType::DiagnosticSessionControl.to_request_sid();

    /// `ECUReset` (ISO 14229-5:2022 REQ 7.10, REQ 7.11).
    pub(crate) const ECU_RESET: u8 = UdsServiceType::EcuReset.to_request_sid();

    /// The positive response to `ECUReset` (REQ 7.11).
    pub(crate) const ECU_RESET_RESPONSE: u8 = UdsServiceType::EcuReset.to_response_sid();
}

/// What ISO 14229-5 clause 8 requires of the connection once this message has
/// been sent.
///
/// Keyed on [`service_ids`] and on what the server says follows the message. The
/// request and response identifiers are disjoint, so no assumption about this
/// transport's role is needed to tell them apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionAction {
    /// Nothing follows. The connection continues to serve the session.
    Continue,
    /// REQ 7.8, REQ 7.10 — a client sent `DiagnosticSessionControl` or
    /// `ECUReset`, so the server will close and a close arriving now is
    /// prescribed rather than a fault.
    ExpectClose,
    /// REQ 7.11 — a server sent a positive `ECUReset` response — or REQ 7.9 — a
    /// server sent the positive `DiagnosticSessionControl` response to a session
    /// change that leaves its running software — and **must itself initiate** the
    /// close, after the response and before executing the service.
    InitiateClose,
}

impl ConnectionAction {
    /// Whether a close arriving now is one the standard prescribes.
    ///
    /// This is `uds_services::TransportEvent::Closed`'s `expected` flag. True
    /// for both non-`Continue` cases: a client awaiting the server's close and
    /// a server that has just caused one are both in a prescribed flow, and the
    /// driver's decision — end the exchange rather than treat it as a failure —
    /// is the same either way.
    pub(crate) const fn close_is_prescribed(self) -> bool {
        matches!(self, Self::ExpectClose | Self::InitiateClose)
    }
}

/// Classify a message this transport is about to send, by its first octet and
/// by what the server says follows it.
///
/// See [`service_ids`] for why the first octet is the service identifier on
/// `DoIP` specifically.
///
/// # Why this crate reads the octet for one close and is told the other
///
/// REQ 7.11 is ISO 14229-5 clause 8, which is this crate's scope, and its close
/// follows every positive `ECUReset` response. Passing it across the transport
/// seam would move the requirement rather than the plumbing: to set such a flag,
/// a driver must know that clause 8 demands a close, which is knowledge
/// ISO 14229-1 does not give it. `simple_doip` cannot
/// decide it either — recognising a service identifier is exactly what
/// `ARCHITECTURE.md` §13 invariant 2 forbids it.
///
/// REQ 7.9's condition is the other way round. Whether a session change leaves
/// the software the server is running is a fact about the server, which only
/// the server has, so the server states it as [`AfterSend::ServerLeaves`] on
/// exactly the message the close follows; clause 8 then decides, here, what
/// follows from it.
///
/// # Why a close is classified, never predicted
///
/// An earlier shape was `post_exchange(request_sid, response) -> PostExchange`,
/// a public function returning `Continue` or `ReconnectAndReactivate` once an
/// exchange finished. It had to answer a question clause 8 does not settle:
/// REQ 7.11 describes the close as following a *positive* response
/// and say nothing about a negative one, so predicting a close meant guessing
/// what a negative response implies.
///
/// Keyed on what was sent, that question does not arise.
/// [`ExpectClose`](ConnectionAction::ExpectClose) says a close would be
/// prescribed, and classifies one that happens;
/// [`InitiateClose`](ConnectionAction::InitiateClose) fires only on a positive
/// response, because a negative one has first octet `0x7F` and lands in
/// [`Continue`](ConnectionAction::Continue) without a rule being needed.
///
/// A response-pending behaves for the same reason: `0x7F` with NRC `0x78` is
/// `Continue`, and the final positive response that follows is what triggers
/// the close. And a request carrying the suppress-positive-response bit
/// produces no response at all, so nothing is sent, nothing is classified, and
/// no close is triggered — which is what REQ 7.11 requires, since it keys on
/// *having sent* a positive response.
///
/// # The client half is deliberately not exact
///
/// [`ExpectClose`](ConnectionAction::ExpectClose) does not read the response,
/// so a negative response to one of these two services followed by an unrelated
/// drop is still reported as expected. Reading it would make the classification
/// exact at the cost of `0x7F` and `0x78` handling here, and the two errors are
/// not symmetric: reporting a close as prescribed when it was not costs a
/// reconnect on an exchange that had already failed, where reporting a
/// prescribed close as unexpected makes a driver fail a flow the standard
/// requires.
#[must_use]
pub(crate) const fn after_sending(first_octet: u8, after: AfterSend) -> ConnectionAction {
    if let AfterSend::ServerLeaves = after {
        return ConnectionAction::InitiateClose;
    }
    match first_octet {
        service_ids::DIAGNOSTIC_SESSION_CONTROL | service_ids::ECU_RESET => {
            ConnectionAction::ExpectClose
        }
        service_ids::ECU_RESET_RESPONSE => ConnectionAction::InitiateClose,
        _ => ConnectionAction::Continue,
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionAction, after_sending, bench_reloads, service_ids};
    use uds_services::AfterSend;

    /// The three octets clause 8 keys connection handling on, and what each
    /// requires. Spelled out as literals rather than taken from `service_ids`,
    /// so the constants and the rule cannot drift together — if
    /// `uds_protocol`'s derivation ever moved, this fails rather than agreeing
    /// with itself.
    #[test]
    fn clause_8_keys_on_three_octets() {
        assert_eq!(service_ids::DIAGNOSTIC_SESSION_CONTROL, 0x10);
        assert_eq!(service_ids::ECU_RESET, 0x11);
        assert_eq!(service_ids::ECU_RESET_RESPONSE, 0x51);

        assert_eq!(
            after_sending(0x10, AfterSend::Continue),
            ConnectionAction::ExpectClose
        );
        assert_eq!(
            after_sending(0x11, AfterSend::Continue),
            ConnectionAction::ExpectClose
        );
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

    /// Nothing else in the byte range means anything to this crate.
    ///
    /// The exhaustive half of the invariant that `uds_on_ip` never learns what
    /// a service is: 253 of the 256 possible first octets are `Continue`, and
    /// the three that are not are the ones the standard itself forces.
    ///
    /// Verified by watching it fail: adding `0x22` — `ReadDataByIdentifier`, an
    /// ordinary service with no connection handling — to `after_sending`'s arm
    /// breaks this test.
    #[test]
    fn every_other_octet_is_continue() {
        for octet in 0x00..=0xFF_u8 {
            if matches!(octet, 0x10 | 0x11 | 0x51) {
                continue;
            }
            assert_eq!(
                after_sending(octet, AfterSend::Continue),
                ConnectionAction::Continue,
                "{octet:#04X} is not a service clause 8 keys connection handling on",
            );
        }
    }

    /// The request and response identifiers are disjoint, which is what makes
    /// one read of the first octet correct for both roles.
    ///
    /// Without this, telling `ExpectClose` from `InitiateClose` would need to
    /// know whether this transport is serving a client or a server — a fact
    /// `DoIpTransport` does not hold and `uds_services` does not put on the
    /// seam. The disjointness makes the question unnecessary rather than
    /// answered, so there is no role assumption for anything to enforce.
    #[test]
    fn a_request_identifier_is_never_a_response_identifier() {
        let requests = [
            service_ids::DIAGNOSTIC_SESSION_CONTROL,
            service_ids::ECU_RESET,
        ];
        let responses = [service_ids::ECU_RESET_RESPONSE];
        for request in requests {
            assert!(
                !responses.contains(&request),
                "{request:#04X} would be classified by role rather than by value",
            );
        }
    }

    /// A negative response triggers no close, and needs no rule to say so.
    ///
    /// REQ 7.9 and REQ 7.11 key the close on a *positive* response. A negative
    /// one has first octet `0x7F` whatever the service, so it falls to
    /// `Continue` without this crate reading an NRC or knowing what one is.
    /// `0x78` — response-pending — is the case that would otherwise be
    /// dangerous: it is not a final response, and closing after it would end an
    /// exchange the server is still working on.
    #[test]
    fn a_negative_response_closes_nothing() {
        assert_eq!(
            after_sending(0x7F, AfterSend::Continue),
            ConnectionAction::Continue
        );
    }

    /// Both prescribed cases produce `expected`, and only those two.
    ///
    /// A client awaiting the server's close and a server that has just caused
    /// one are both in a flow the standard requires, and the driver's decision
    /// is the same for both: end the exchange rather than report a failure.
    #[test]
    fn only_a_prescribed_flow_reports_expected() {
        assert!(ConnectionAction::ExpectClose.close_is_prescribed());
        assert!(ConnectionAction::InitiateClose.close_is_prescribed());
        assert!(
            !ConnectionAction::Continue.close_is_prescribed(),
            "a drop during an ordinary exchange is not prescribed by anything",
        );
    }

    /// The bench values are the `tP6` pair, and they are the ones a reader of
    /// ISO 14229-2:2021 clause 9.2 Table 4 would recognise as conventional.
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
