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
/// what a service is. These two constants are the exception the standard itself forces, and
/// the exhaustive list of it.
///
/// ISO 14229-5:2022 REQ 7.8–7.11 make TCP connection handling part of the
/// `DiagnosticSessionControl` and `ECUReset` flows specifically: the server closes
/// the connection after its positive response and before executing the service,
/// and the client must establish a new connection *and repeat routing
/// activation* before continuing. That is transport behaviour keyed on a
/// service identifier, and clause 8 puts it in the IP profile rather than in
/// ISO 14229-1.
///
/// So the invariant is narrower than first written: this crate knows these two
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
/// standards rather than a property a test can hold: it becomes testable
/// when `post_exchange` is implemented and can be exercised against a real
/// `T_PDU`.
pub(crate) mod service_ids {
    use uds_protocol::UdsServiceType;

    /// `DiagnosticSessionControl` (ISO 14229-5:2022 REQ 7.8, REQ 7.9).
    pub(crate) const DIAGNOSTIC_SESSION_CONTROL: u8 =
        UdsServiceType::DiagnosticSessionControl.to_request_sid();

    /// `ECUReset` (ISO 14229-5:2022 REQ 7.10, REQ 7.11).
    pub(crate) const ECU_RESET: u8 = UdsServiceType::EcuReset.to_request_sid();
}

/// Whether a close arriving now is part of a flow the standard prescribes.
///
/// `request_sid` is the service identifier of the request this transport last
/// sent — its first octet; see [`service_ids`] for why that is the service
/// identifier on `DoIP` specifically.
///
/// # Why this classifies a close rather than predicting one
///
/// An earlier shape was `post_exchange(request_sid, response) -> PostExchange`,
/// a public function returning `Continue` or `ReconnectAndReactivate` once an
/// exchange finished. It had to answer a question clause 8 does not settle:
/// REQ 7.9 and REQ 7.11 describe the close as following a *positive* response,
/// and say nothing about a negative one, so predicting a close meant guessing
/// what a negative response implies and recording the guess as an open question
/// against the requirement set.
///
/// Keyed on a close that has already happened, that question does not arise.
/// This never predicts anything: `uds_services::TransportEvent::Closed` reports
/// a close the transport observed, and all this decides is whether it was
/// plausibly prescribed. A negative response needs no ruling because nothing
/// asks for one.
///
/// # This is deliberately not exact
///
/// It does not read the response at all, so a negative response to one of these
/// two services followed by an unrelated drop is reported as expected. Reading
/// the response would make it exact, at the cost of a third piece of ISO 14229-1
/// vocabulary here — the `0x7F` negative-response format, and
/// `requestCorrectlyReceived-ResponsePending` (0x78), which must not disarm a
/// pending close.
///
/// The imprecision is taken deliberately, because the two errors are not
/// symmetric. Reporting `expected` wrongly costs the driver a reconnect and a
/// routing activation on an exchange that had already failed. Reporting an
/// expected close as unexpected makes the driver fail an exchange the standard
/// prescribes — a conformance failure rather than a wasted round trip.
#[must_use]
pub(crate) const fn close_is_expected_after(request_sid: u8) -> bool {
    matches!(
        request_sid,
        service_ids::DIAGNOSTIC_SESSION_CONTROL | service_ids::ECU_RESET
    )
}

#[cfg(test)]
mod tests {
    use super::{bench_reloads, close_is_expected_after, service_ids};

    /// The two services clause 8 makes TCP connection handling part of, and
    /// nothing else. `0x10` and `0x11` are spelled out rather than taken from
    /// `service_ids`, so the constants and the rule cannot drift together.
    #[test]
    fn only_the_two_services_clause_8_names_expect_a_close() {
        assert!(close_is_expected_after(0x10), "DiagnosticSessionControl");
        assert!(close_is_expected_after(0x11), "ECUReset");
        assert_eq!(service_ids::DIAGNOSTIC_SESSION_CONTROL, 0x10);
        assert_eq!(service_ids::ECU_RESET, 0x11);

        for sid in 0x00..=0xFF_u8 {
            if sid == 0x10 || sid == 0x11 {
                continue;
            }
            assert!(
                !close_is_expected_after(sid),
                "{sid:#04X} is not a service clause 8 keys connection handling on",
            );
        }
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
