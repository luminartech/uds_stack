//! Application layer: the `UDSonIP` profile.
//!
//! ISO 14229-5:2022 clause 8, REQ 7.1–7.20. This is the half of the crate that
//! sits *above* the session layer, and it is where the genuinely IP-specific
//! behaviour lives: how a UDS message is framed into a `DoIP` diagnostic message,
//! and what the transport must do around particular services.

/// The timing parameters this transport supplies to the session layer.
///
/// ISO 14229-5:2022 REQ 7.19 defers the values themselves to ISO 14229-2. What
/// this crate contributes is the *choice of parameter*: `DoIP` offers no
/// `T_DataSOM.ind`, so the `tP6` pair applies rather than the `tP2` pair
/// (ISO 14229-2:2021 REQ 5.11).
///
/// ISO 14229-2:2021 clause 9.2 Table 4 specifies these as minima derived from
/// the server's timing and vehicle-network delays, not as fixed values, so
/// there is no defensible universal default. The values here are the
/// conventional starting points for a bench setup and must be configured for a
/// real vehicle.
///
/// Values are milliseconds, matching the unit the session layer's clock uses
/// (ISO 14229-2:2021 clause 9.5 Table 5 states its timing parameters in ms).
/// Carrying `Duration` here would mean converting at the seam on every call,
/// which is a conversion that can only go wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// `tP6_Client_Max` — wait for a complete response after `T_Data.conf`.
    pub p6_client_max_ms: u32,
    /// `tP6*_Client_Max` — enhanced wait after a response-pending NRC.
    pub p6_star_client_max_ms: u32,
    /// `tP3_Client_Phys` — minimum spacing before the next physically
    /// addressed request when the previous one required no response.
    pub p3_client_phys_ms: u32,
    /// `tP3_Client_Func` — the same, for functionally addressed requests.
    pub p3_client_func_ms: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            p6_client_max_ms: 2_000,
            p6_star_client_max_ms: 5_000,
            p3_client_phys_ms: 0,
            p3_client_func_ms: 0,
        }
    }
}

/// Service identifiers whose *transport* behaviour ISO 14229-5 specifies.
///
/// # Why this crate knows any service identifier at all
///
/// `ARCHITECTURE.md` §11 states that `uds_on_ip` never learns what a service
/// is. These two constants are the exception the standard itself forces, and
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
pub mod service_ids {
    use uds_protocol::UdsServiceType;

    /// `DiagnosticSessionControl` (ISO 14229-5:2022 REQ 7.8, REQ 7.9).
    pub const DIAGNOSTIC_SESSION_CONTROL: u8 =
        UdsServiceType::DiagnosticSessionControl.to_request_sid();

    /// `ECUReset` (ISO 14229-5:2022 REQ 7.10, REQ 7.11).
    pub const ECU_RESET: u8 = UdsServiceType::EcuReset.to_request_sid();
}

/// What the transport must do once an exchange completes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostExchange {
    /// Nothing; the connection continues to serve the session.
    Continue,
    /// Expect the server to close, then re-establish the connection and repeat
    /// routing activation before further diagnostic communication
    /// (ISO 14229-5:2022 REQ 7.8, REQ 7.10).
    ReconnectAndReactivate,
}

/// Decide what must happen to the connection after this exchange.
///
/// Keyed only on the request's service identifier and whether the response was
/// positive, per REQ 7.8 and REQ 7.10.
///
/// # Prototype note
///
/// Whether a *negative* response also implies a close is not stated by clause 8
/// as directly as the positive case is; REQ 7.9 and REQ 7.11 both describe the
/// close as following a *positive* response. Treating a negative response as
/// `Continue` is the reading taken here and is a candidate open question for
/// the requirement set.
#[allow(unused_variables)]
#[must_use]
pub fn post_exchange(request: &[u8], response: &[u8]) -> PostExchange {
    todo!("REQ 7.8 / REQ 7.10 — key on service_ids and response polarity")
}

/// Whether a periodic data record fits the non-segmented message limit.
///
/// ISO 14229-5:2022 REQ 7.17 requires that the record referenced by a periodic
/// data identifier not exceed the length limit of a non-segmented `UDSonIP`
/// message.
#[allow(unused_variables)]
#[must_use]
pub fn periodic_record_within_limit(len: usize) -> bool {
    todo!("REQ 7.17 — bound against the non-segmented UDSonIP message limit")
}
