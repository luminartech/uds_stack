//! Diagnostics and communication management — ISO 14229-1:2020 clause 10.
//!
//! ``UDSSVC_ARCH_0012`` — one trait per service.

use crate::{ResponseSink, SessionTransition};
use uds_protocol::{
    CommunicationControlType, CommunicationType, DiagnosticSessionType, DtcSettingType,
    NegativeResponseCode, ResetType, SubnetNumber,
};

/// The `P2` pair a session advertises in its `DiagnosticSessionControl` response.
///
/// ISO 14229-1:2020 clause 10.2 — the positive response carries `P2Server_max` and
/// `P2*Server_max` for the session being entered, so they are a property of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionTiming {
    /// `P2Server_max` in milliseconds.
    pub p2_server_max: u32,
    /// `P2*Server_max` in milliseconds.
    pub p2_star_server_max: u32,
}

/// `DiagnosticSessionControl` (0x10).
///
/// ``UDSSVC_ARCH_0035`` puts the active session in this crate and ``UDSSVC_ARCH_0038``
/// classifies the change before the application sees it, so an application never tracks
/// a session itself.
///
/// **No `MAY_RESPOND_PENDING`**, unlike the services in ``UDSSVC_ARCH_0033``. A
/// response-pending is what the driver sends while it is still awaiting a handler, and
/// nothing here is awaited: [`Self::supports`] and [`Self::timing`] are lookups the
/// pipeline makes before composing the response, and [`Self::on_transition`] runs after
/// that response has gone out. There is no window in which a 0x78 could come due, so the
/// constant would have had one possible value and no effect.
pub trait DiagnosticSessionControl {
    /// The longest positive response beyond the mandatory four timing bytes.
    const MAX_RESPONSE_LEN: usize;

    /// Whether this server supports `session`.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being requested; see [`DiagnosticSessionType`], whose
    ///   reserved and manufacturer-specific variants carry the raw byte for a server
    ///   that defines its own.
    fn supports(&self, session: DiagnosticSessionType) -> bool;

    /// The `P2` pair to advertise for `session`.
    ///
    /// **Must equal the session layer's [`ServerParams`](crate::ServerParams) for
    /// `session`.** The response advertises these, and the session layer enforces its
    /// own parameters, so the two have separate sources. Milestone 1 does not apply the
    /// confirmed session's timing to the session layer (`set_parameter`); that is a
    /// follow-up.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being entered; see [`DiagnosticSessionType`].
    fn timing(&self, session: DiagnosticSessionType) -> SessionTiming;

    /// Called after the positive response, and on `tS3_Server` expiry.
    ///
    /// ``UDSSVC_ARCH_0038``. `security_relocked` is true where the transition relocked a
    /// security level, so functionality gated on it can be dropped — clause 10.4 makes a
    /// session change lock every level.
    fn on_transition(&mut self, transition: SessionTransition, security_relocked: bool);
}

/// `EcuReset` (0x11).
pub trait EcuReset {
    /// ``UDSSVC_ARCH_0033`` — a reset legitimately sets this.
    const MAY_RESPOND_PENDING: bool;

    /// Perform `kind`, writing any `powerDownTime` into `out`.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset to perform; see [`ResetType`].
    /// * `out` - where the `powerDownTime` byte goes, for the reset that carries one.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported or impermissible reset.
    fn reset(
        &mut self,
        kind: ResetType,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `TesterPresent` (0x3E).
///
/// Restarting `tS3_Server` is deliberately absent: ISO 14229-2 puts it in the session
/// layer and ``UDSSVC_ARCH_0002`` keeps it there. This exists for servers that act on it.
///
/// **No `MAY_RESPOND_PENDING`**, for the reason
/// [`DiagnosticSessionControl`] has none: [`Self::on_tester_present`] is synchronous, so
/// the driver is never awaiting it when a deadline passes and no 0x78 can become due.
/// Which is as well — a server too busy to answer the message whose only purpose is to
/// say it is still there has a larger problem than a response-pending.
pub trait TesterPresent {
    /// Called on each accepted `TesterPresent`.
    fn on_tester_present(&mut self);
}

/// `CommunicationControl` (0x28).
pub trait CommunicationControl {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Apply `control_type` to `communication_type` on `node`.
    ///
    /// # Arguments
    ///
    /// * `control_type` - what to do; see [`CommunicationControlType`].
    /// * `communication_type` - what it applies to; see [`CommunicationType`].
    /// * `node` - which network it applies to; see [`SubnetNumber`], which distinguishes
    ///   [`SubnetNumber::ReceivedOn`] from [`SubnetNumber::AllConnectedNetworks`].
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported combination.
    fn control(
        &mut self,
        control_type: CommunicationControlType,
        communication_type: CommunicationType,
        node: SubnetNumber,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ControlDTCSetting` (0x85).
pub trait ControlDtcSetting {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The longest `DTCSettingControlOptionRecord` this server accepts.
    ///
    /// Clause 10.7 leaves the record manufacturer-specific and gives it no fixed width,
    /// so the ceiling is the application's to state — and stating it is what puts this
    /// service's real contribution into the derived in-flight buffer rather than the
    /// catch-all's six bytes.
    const MAX_OPTION_RECORD_LEN: usize;

    /// Apply `setting`, with the manufacturer-specific option record.
    ///
    /// # Arguments
    ///
    /// * `setting` - whether DTC status updates are on or off; see [`DtcSettingType`].
    /// * `option_record` - at most [`Self::MAX_OPTION_RECORD_LEN`] bytes; a request
    ///   carrying more is rejected before it reaches here.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported setting.
    fn control_dtc_setting(
        &mut self,
        setting: DtcSettingType,
        option_record: &[u8],
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

#[cfg(test)]
#[allow(
    clippy::manual_range_patterns,
    clippy::unused_async_trait_impl,
    reason = "the fixture names sessions as an explicit OR-set to mirror the brief's \
              uninterpreted sub-function values, and its reset never awaits — neither is \
              a real defect in test code"
)]
mod tests {
    use super::{DiagnosticSessionControl, EcuReset, SessionTiming};
    use crate::{ResponseSink, SessionTransition};
    use uds_protocol::{DiagnosticSessionType, NegativeResponseCode, ResetType};

    struct Ecu;

    impl DiagnosticSessionControl for Ecu {
        const MAX_RESPONSE_LEN: usize = 0;
        fn supports(&self, session: DiagnosticSessionType) -> bool {
            matches!(
                session,
                DiagnosticSessionType::DefaultSession
                    | DiagnosticSessionType::ProgrammingSession
                    | DiagnosticSessionType::ExtendedDiagnosticSession
            )
        }
        fn timing(&self, _s: DiagnosticSessionType) -> SessionTiming {
            SessionTiming {
                p2_server_max: 50,
                p2_star_server_max: 5_000,
            }
        }
        fn on_transition(&mut self, _t: SessionTransition, _relocked: bool) {}
    }

    impl EcuReset for Ecu {
        const MAY_RESPOND_PENDING: bool = true;
        async fn reset(
            &mut self,
            _k: ResetType,
            _o: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
    }

    /// ``UDSSVC_ARCH_0038`` — the application receives a classified transition, not a
    /// sub-function value. Which of Figure 7's four cases occurred determines what
    /// configuration state resets, and classifying it here stops every application
    /// reimplementing Table 23.
    #[test]
    fn the_application_receives_a_classified_transition() {
        let mut ecu = Ecu;
        ecu.on_transition(SessionTransition::NonDefaultToDefault, true);
        assert!(ecu.supports(DiagnosticSessionType::ProgrammingSession));
        assert!(!ecu.supports(DiagnosticSessionType::SafetySystemDiagnosticSession));
    }

    /// P2 values are a property of the session being entered, so the application states
    /// them and this crate composes the response.
    #[test]
    fn a_session_carries_its_own_timing() {
        let t = Ecu.timing(DiagnosticSessionType::ProgrammingSession);
        assert_eq!((t.p2_server_max, t.p2_star_server_max), (50, 5_000));
    }
}
