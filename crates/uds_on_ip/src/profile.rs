//! Application layer: the `UDSonIP` profile.
//!
//! ISO 14229-5:2022 clause 8, REQ 7.1–7.20. This is the half of the crate that
//! sits *above* the session layer, and it is where the genuinely IP-specific
//! behaviour lives: how a UDS message is framed into a `DoIP` diagnostic message,
//! and what the transport must do around particular services.

/// The `tP_Client` reload pair this transport dictates.
///
/// ISO 14229-5:2022 REQ 7.19 defers the values to ISO 14229-2. What this crate
/// contributes is the *choice of parameter*: `DoIP` offers no
/// `T_DataSOM.ind`, so the `tP6` pair applies rather than the `tP2` pair
/// (ISO 14229-2:2021 REQ 5.11).
///
/// This is what `UdsTransport::channel_timing` supplies. The spacing pair is
/// deliberately not part of it — see [`Spacing`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reloads {
    /// `tP6_Client_Max` — wait for a complete response after `T_Data.conf`.
    pub p6_client_max_ms: u32,
    /// `tP6*_Client_Max` — enhanced wait after a response-pending NRC.
    pub p6_star_client_max_ms: u32,
}

impl Reloads {
    /// The value for whichever reload the session layer says is in force.
    #[must_use]
    pub const fn value_for(self, which: uds_session::ChannelReload) -> u32 {
        match which {
            uds_session::ChannelReload::Default => self.p6_client_max_ms,
            uds_session::ChannelReload::Enhanced => self.p6_star_client_max_ms,
        }
    }
}

/// Minimum spacing between consecutive requests.
///
/// ISO 14229-2:2021 clause 9.7 client policy. Separate from [`Reloads`]
/// because a transport has no view on it: the reload pair is dictated by
/// whether the transport offers a `T_DataSOM.ind`, while the spacing is the
/// client's own conduct.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Spacing {
    /// `tP3_Client_Phys` — minimum spacing before the next physically
    /// addressed request when the previous one required no response.
    pub p3_client_phys_ms: u32,
    /// `tP3_Client_Func` — the same, for functionally addressed requests.
    pub p3_client_func_ms: u32,
}

/// Every ISO 14229-5 timing parameter this profile carries.
///
/// ISO 14229-2:2021 clause 9.2 Table 4 specifies the reloads as minima derived
/// from the server's timing and vehicle-network delays, not as fixed values,
/// so there is no defensible universal default. The values in [`Timing::default`]
/// are conventional bench starting points and must be configured for a real
/// vehicle.
///
/// Values are milliseconds, matching `uds_session::Timestamp`'s unit. Carrying
/// a `Duration` here would mean converting at the seam on every call, which is
/// a conversion that can only go wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// What the session layer loads its response timer with.
    pub reloads: Reloads,
    /// What the client waits before its next request.
    pub spacing: Spacing,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            reloads: Reloads {
                p6_client_max_ms: 2_000,
                p6_star_client_max_ms: 5_000,
            },
            spacing: Spacing::default(),
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

#[cfg(test)]
mod tests {
    use super::{Reloads, Spacing, Timing};
    use uds_session::ChannelReload;

    /// ISO 14229-2:2021 REQ 5.11 — `DoIP` has no `T_DataSOM.ind`, so the `tP6`
    /// pair applies rather than the `tP2` pair. The session layer does not
    /// distinguish the two cases; the distinction survives in the values this
    /// transport supplies.
    #[test]
    fn the_reload_pair_answers_the_session_layers_question() {
        let reloads = Reloads {
            p6_client_max_ms: 2_000,
            p6_star_client_max_ms: 5_000,
        };
        assert_eq!(reloads.value_for(ChannelReload::Default), 2_000);
        assert_eq!(reloads.value_for(ChannelReload::Enhanced), 5_000);
    }

    /// Design doc §6.2 — spacing is ISO 14229-2 clause 9.7 client policy, on
    /// which a transport has no view, so it is not part of what
    /// `channel_timing` supplies.
    #[test]
    fn spacing_is_separable_from_the_reloads() {
        let timing = Timing::default();
        let reloads: Reloads = timing.reloads;
        let spacing: Spacing = timing.spacing;
        assert_eq!(reloads, timing.reloads);
        assert_eq!(spacing, timing.spacing);
    }
}
