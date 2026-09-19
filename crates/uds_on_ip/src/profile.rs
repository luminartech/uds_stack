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
pub mod service_ids {
    use uds_protocol::UdsServiceType;

    /// `DiagnosticSessionControl` (ISO 14229-5:2022 REQ 7.8, REQ 7.9).
    pub const DIAGNOSTIC_SESSION_CONTROL: u8 =
        UdsServiceType::DiagnosticSessionControl.to_request_sid();

    /// `ECUReset` (ISO 14229-5:2022 REQ 7.10, REQ 7.11).
    pub const ECU_RESET: u8 = UdsServiceType::EcuReset.to_request_sid();
}

/// What the transport must do once an exchange completes.
///
/// Whether a *negative* response also implies a close is not stated by clause 8
/// as directly as the positive case is: REQ 7.9 and REQ 7.11 both describe the
/// close as following a *positive* response. Treating a negative response as
/// [`Continue`](PostExchange::Continue) is the reading taken here, and is a
/// candidate open question for the requirement set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostExchange {
    /// Nothing; the connection continues to serve the session.
    Continue,
    /// Expect the server to close, then re-establish the connection and repeat
    /// routing activation before further diagnostic communication
    /// (ISO 14229-5:2022 REQ 7.8, REQ 7.10).
    ReconnectAndReactivate,
}

/// Whether the server answered positively.
///
/// A `bool` would do the same work and read as `post_exchange(sid, true)` at
/// the call site, where `true` says nothing about which way round the question
/// was asked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponsePolarity {
    /// A positive response.
    Positive,
    /// A negative response, carrying an NRC this crate does not read.
    Negative,
}

/// Decide what must happen to the connection after this exchange.
///
/// Keyed on the request's service identifier and the response's polarity, per
/// REQ 7.8 and REQ 7.10 — which is all clause 8 keys it on, so those are the
/// only two things this takes.
///
/// An earlier shape took the request and response as two `&[u8]`. Two
/// same-typed slices in one signature can be passed the wrong way round, and
/// swapping them silently returns the wrong answer for the most consequential
/// question this module asks: whether the connection is about to close under
/// the caller. A `u8` and an enum cannot be swapped. The caller reads the
/// service identifier from the request's first octet — see [`service_ids`] for
/// why that is the service identifier on `DoIP` specifically.
#[expect(
    unused_variables,
    reason = "both are unused until post_exchange's body replaces the todo!() below"
)]
#[must_use]
pub fn post_exchange(request_sid: u8, response: ResponsePolarity) -> PostExchange {
    todo!("REQ 7.8 / REQ 7.10 — key on service_ids and response polarity")
}

#[cfg(test)]
mod tests {
    use super::bench_reloads;

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
