//! Diagnostics and communication management — ISO 14229-1:2020 clause 10.
//!
//! ``UDSSVC_ARCH_0012`` — one trait per service.

use crate::{ResponseSink, SecurityLevel, SessionTransition};
use uds_protocol::{
    CommunicationControlType, CommunicationType, DiagnosticSessionType, DtcSettingType,
    NegativeResponseCode, ResetType, SubnetNumber,
};

/// The `P2` pair a session advertises in its `DiagnosticSessionControl` response, and
/// that [`crate::Server`] enforces while the session is in force.
///
/// ISO 14229-1:2020 clause 10.2 — the positive response carries `P2Server_max` and
/// `P2*Server_max` for the session being entered, so they are a property of the session.
/// Held in Table 29's wire form, so every value is advertised exactly and the window
/// enforced is the one advertised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionTiming {
    /// `P2Server_max`, in 1 ms units.
    pub p2_server_max_ms: u16,
    /// `P2*Server_max`, in 10 ms units.
    pub p2_star_server_max_10ms: u16,
}

impl SessionTiming {
    /// `P2Server_max` in milliseconds, the session layer's unit.
    pub(crate) fn p2_server_max(self) -> u32 {
        u32::from(self.p2_server_max_ms)
    }

    /// `P2*Server_max` in milliseconds, the session layer's unit.
    pub(crate) fn p2_star_server_max(self) -> u32 {
        u32::from(self.p2_star_server_max_10ms) * 10
    }
}

/// `DiagnosticSessionControl` (0x10).
///
/// ``UDSSVC_ARCH_0035`` puts the active session in this crate and ``UDSSVC_ARCH_0038``
/// classifies the change before the application sees it, so an application never tracks
/// a session itself.
///
/// **No `MAY_RESPOND_PENDING`**, unlike the services in ``UDSSVC_ARCH_0033``. A
/// response-pending is what the driver sends while it is still awaiting a handler, and
/// nothing here is awaited: [`Self::supports`], [`Self::supported_from`] and
/// [`Self::timing`] are lookups the pipeline makes before composing the response, and
/// [`Self::on_transition`] runs after that response has gone out. There is no window in
/// which a 0x78 could come due, so the constant would have had one possible value and no
/// effect.
pub trait DiagnosticSessionControl {
    /// The longest positive response beyond the mandatory four timing bytes.
    const MAX_RESPONSE_LEN: usize;

    /// Whether this server supports `session` at all, from whichever session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported ever for the SID?" —
    /// ``UDSSVC_ARCH_0007`` row 2. A `false` settles the request
    /// `subFunctionNotSupported` (0x12) before [`Self::supported_from`] is asked.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being requested; see [`DiagnosticSessionType`], whose
    ///   reserved and manufacturer-specific variants carry the raw byte for a server
    ///   that defines its own.
    fn supports(&self, session: DiagnosticSessionType) -> bool;

    /// Whether `session` may be entered from `active`.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported in active session
    /// for the SID?" — ``UDSSVC_ARCH_0007`` row 4. Asked only for a `session` that
    /// [`Self::supports`] accepted, so a `false` here is always "supported, but not from
    /// this session": the pipeline settles it `subFunctionNotSupportedInActiveSession`
    /// (0x7E), which Annex A reserves for a sub-function "known to be supported in
    /// another session". A `session` this server never supports is 0x12, however this
    /// answers.
    ///
    /// No default: the table of which session is reachable from which is the
    /// application's to state. A server that restricts no transition returns `true`.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being requested; see [`DiagnosticSessionType`].
    /// * `active` - the session the server is in when the request arrives.
    fn supported_from(
        &self,
        session: DiagnosticSessionType,
        active: DiagnosticSessionType,
    ) -> bool;

    /// The `P2` pair `session` advertises and, while it is in force, the server enforces.
    ///
    /// Asked for the entered session when composing the positive response, and for the
    /// session in force each time a request arrives: [`crate::Server`] loads the pair
    /// into the session layer then, so the request's window is the one its session
    /// advertised, and any session the server is in — the default one included — is
    /// timed by this and not by the [`ServerParams`](crate::ServerParams) passed to
    /// [`Server::new`](crate::Server::new). The returned pair must keep those params'
    /// `response_pending_lead` well formed
    /// ([`ServerParams::is_well_formed`](crate::ServerParams::is_well_formed)); a debug
    /// build panics on the first request timed by a pair that does not.
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
///
/// ISO 14229-1:2020 clause 10.3. The pipeline writes the positive response's service
/// identifier and echoed `resetType`; [`Self::reset`] decides whether the reset is
/// accepted and supplies the `powerDownTime` that follows.
pub trait EcuReset {
    /// ``UDSSVC_ARCH_0033`` — a reset legitimately sets this.
    const MAY_RESPOND_PENDING: bool;

    /// Whether this server supports `kind` at all, in whichever session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported ever for the SID?" —
    /// ``UDSSVC_ARCH_0007`` row 2. A `false` settles the request
    /// `subFunctionNotSupported` (0x12) before [`Self::supported_in`] is asked.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested; see [`ResetType`], whose reserved and specific
    ///   variants carry the raw byte for a server that defines its own.
    fn supports(&self, kind: ResetType) -> bool;

    /// Whether `kind` is available in the `active` session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported in active session
    /// for the SID?" — ``UDSSVC_ARCH_0007`` row 4. Asked only for a `kind` that
    /// [`Self::supports`] accepted, so a `false` settles the request
    /// `subFunctionNotSupportedInActiveSession` (0x7E). A server that restricts no reset
    /// to a session returns `true`.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested; see [`ResetType`].
    /// * `active` - the session the server is in when the request arrives.
    fn supported_in(&self, kind: ResetType, active: DiagnosticSessionType) -> bool;

    /// The security level `kind` requires unlocked, or `None` where it requires none.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` security check OK?" — asked only
    /// for a `kind` that [`Self::supported_in`] accepted. Where the level this crate holds
    /// unlocked is not the one returned, the request settles `securityAccessDenied`
    /// (0x33) without [`Self::reset`] being asked.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested; see [`ResetType`].
    fn required_level(&self, kind: ResetType) -> Option<SecurityLevel>;

    /// Accept or refuse `kind`, writing any `powerDownTime` into `out`.
    ///
    /// **Must not perform the reset before returning.** ISO 14229-1:2020 clause 10.3.1
    /// strongly recommends that the positive response is sent before the reset is
    /// executed, and it is sent only after this returns `Ok`.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested, one [`Self::supports`] and [`Self::supported_in`]
    ///   accepted and whose [`Self::required_level`] is unlocked; see [`ResetType`].
    /// * `out` - where the `powerDownTime` byte goes, for
    ///   [`ResetType::EnableRapidPowerShutDown`], the one reset whose response carries
    ///   it (clause 10.3.3, Table 35); nothing is written for any other.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a reset whose criteria are not met, such as
    /// `conditionsNotCorrect` (0x22), clause 10.3.4. A locked level is
    /// [`Self::required_level`]'s, not this method's.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
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
///
/// **No sub-function lookups**, unlike [`DiagnosticSessionControl`]'s
/// [`supports`](DiagnosticSessionControl::supports) and
/// [`supported_from`](DiagnosticSessionControl::supported_from). The service's only
/// sub-function is `zeroSubFunction`, which ISO 14229-1:2020 clause 10.7 makes mandatory,
/// and clause 10.2's Table 23 makes the service available in the default and every
/// non-default session. So Figure 6's "supported ever" and "supported in active session"
/// (``UDSSVC_ARCH_0007`` rows 2 and 4) have nothing deployment-specific to ask: the
/// pipeline answers row 2 from the byte, and row 4 is always yes.
pub trait TesterPresent {
    /// Called on each accepted `TesterPresent`.
    fn on_tester_present(&mut self);
}

/// `CommunicationControl` (0x28).
pub trait CommunicationControl {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Whether this server supports `control_type` at all, in whichever session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported ever for the SID?" —
    /// ``UDSSVC_ARCH_0007`` row 2. A `false` settles the request
    /// `subFunctionNotSupported` (0x12) before [`Self::supported_in`] is asked.
    ///
    /// # Arguments
    ///
    /// * `control_type` - the `controlType` requested; see [`CommunicationControlType`],
    ///   whose reserved and specific variants carry the raw byte.
    fn supports(&self, control_type: CommunicationControlType) -> bool;

    /// Whether `control_type` is available in the `active` session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported in active session
    /// for the SID?" — ``UDSSVC_ARCH_0007`` row 4. Asked only for a `control_type` that
    /// [`Self::supports`] accepted, so a `false` settles the request
    /// `subFunctionNotSupportedInActiveSession` (0x7E). The default session never reaches
    /// here: Table 23 refuses the service there with 0x7F.
    ///
    /// # Arguments
    ///
    /// * `control_type` - the `controlType` requested; see [`CommunicationControlType`].
    /// * `active` - the session the server is in when the request arrives; see
    ///   [`DiagnosticSessionType`].
    fn supported_in(
        &self,
        control_type: CommunicationControlType,
        active: DiagnosticSessionType,
    ) -> bool;

    /// The security level `control_type` requires unlocked, or `None` where it requires
    /// none.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` security check OK?" — asked only
    /// for a `control_type` that [`Self::supported_in`] accepted. Where the level this
    /// crate holds unlocked is not the one returned, the request settles
    /// `securityAccessDenied` (0x33) without [`Self::control`] being asked.
    ///
    /// # Arguments
    ///
    /// * `control_type` - the `controlType` requested; see [`CommunicationControlType`].
    fn required_level(
        &self,
        control_type: CommunicationControlType,
    ) -> Option<SecurityLevel>;

    /// Apply `control_type` to `communication_type` on `subnet`, or on the node `node_id`
    /// names.
    ///
    /// Clause 10.5.1: the positive response is owed even where the requested state is
    /// already in effect.
    ///
    /// # Arguments
    ///
    /// * `control_type` - what to do; see [`CommunicationControlType`].
    /// * `communication_type` - what it applies to; see [`CommunicationType`].
    /// * `subnet` - which network it applies to; see [`SubnetNumber`], which distinguishes
    ///   [`SubnetNumber::ReceivedOn`] from [`SubnetNumber::AllConnectedNetworks`].
    /// * `node_id` - the `nodeIdentificationNumber`, present exactly where `control_type`
    ///   is one of the enhanced-address variants (clause 10.5.2.3; see
    ///   [`CommunicationControlType::is_extended_address_variant`]).
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where the control cannot be applied, clause 10.5.4:
    /// `conditionsNotCorrect` (0x22), or `requestOutOfRange` (0x31) for an error in
    /// `communication_type` or `node_id`.
    fn control(
        &mut self,
        control_type: CommunicationControlType,
        communication_type: CommunicationType,
        subnet: SubnetNumber,
        node_id: Option<u16>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ControlDTCSetting` (0x85).
pub trait ControlDtcSetting {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The longest `DTCSettingControlOptionRecord` this server accepts.
    ///
    /// Clause 10.8.2.3 leaves the record manufacturer-specific and gives it no fixed
    /// width, so the ceiling is the application's to state — and stating it is what puts
    /// this service's real contribution into the derived in-flight buffer rather than the
    /// catch-all's six bytes. A longer record is `incorrectMessageLengthOrInvalidFormat`
    /// (0x13) without [`Self::control_dtc_setting`] being asked.
    const MAX_OPTION_RECORD_LEN: usize;

    /// Whether this server supports `setting` at all, in whichever session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported ever for the SID?" —
    /// ``UDSSVC_ARCH_0007`` row 2. A `false` settles the request
    /// `subFunctionNotSupported` (0x12) before [`Self::supported_in`] is asked. A reserved
    /// `DTCSettingType` is not a [`DtcSettingType`] and is 0x12 without being asked.
    ///
    /// # Arguments
    ///
    /// * `setting` - the `DTCSettingType` requested; see [`DtcSettingType`].
    fn supports(&self, setting: DtcSettingType) -> bool;

    /// Whether `setting` is available in the `active` session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported in active session
    /// for the SID?" — ``UDSSVC_ARCH_0007`` row 4. Asked only for a `setting` that
    /// [`Self::supports`] accepted, so a `false` settles the request
    /// `subFunctionNotSupportedInActiveSession` (0x7E). The default session never reaches
    /// here: Table 23 refuses the service there with 0x7F.
    ///
    /// # Arguments
    ///
    /// * `setting` - the `DTCSettingType` requested; see [`DtcSettingType`].
    /// * `active` - the session the server is in when the request arrives; see
    ///   [`DiagnosticSessionType`].
    fn supported_in(&self, setting: DtcSettingType, active: DiagnosticSessionType) -> bool;

    /// The security level `setting` requires unlocked, or `None` where it requires none.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` security check OK?" — asked only
    /// for a `setting` that [`Self::supported_in`] accepted. Where the level this crate
    /// holds unlocked is not the one returned, the request settles
    /// `securityAccessDenied` (0x33) without [`Self::control_dtc_setting`] being asked.
    ///
    /// # Arguments
    ///
    /// * `setting` - the `DTCSettingType` requested; see [`DtcSettingType`].
    fn required_level(&self, setting: DtcSettingType) -> Option<SecurityLevel>;

    /// Apply `setting`, with the manufacturer-specific option record.
    ///
    /// Clause 10.8.1: the positive response is owed even where `setting` is already in
    /// effect, and updating resumes on a transition to a session where this service is
    /// not supported, which `DiagnosticSessionControl::on_transition` reports.
    ///
    /// # Arguments
    ///
    /// * `setting` - whether DTC status updates are on or off; see [`DtcSettingType`].
    /// * `option_record` - at most [`Self::MAX_OPTION_RECORD_LEN`] bytes; a request
    ///   carrying more is rejected before it reaches here.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where the setting cannot be applied, clause 10.8.4:
    /// `conditionsNotCorrect` (0x22), or `requestOutOfRange` (0x31) for an error in
    /// `option_record`.
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
        fn supported_from(
            &self,
            session: DiagnosticSessionType,
            active: DiagnosticSessionType,
        ) -> bool {
            !matches!(session, DiagnosticSessionType::ProgrammingSession)
                || matches!(active, DiagnosticSessionType::ExtendedDiagnosticSession)
        }
        fn timing(&self, _s: DiagnosticSessionType) -> SessionTiming {
            SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max_10ms: 500,
            }
        }
        fn on_transition(&mut self, _t: SessionTransition, _relocked: bool) {}
    }

    impl EcuReset for Ecu {
        const MAY_RESPOND_PENDING: bool = true;
        fn supports(&self, _k: ResetType) -> bool {
            true
        }
        fn supported_in(&self, _k: ResetType, _a: DiagnosticSessionType) -> bool {
            true
        }
        fn required_level(&self, _k: ResetType) -> Option<crate::SecurityLevel> {
            None
        }
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
        assert!(DiagnosticSessionControl::supports(
            &ecu,
            DiagnosticSessionType::ProgrammingSession
        ));
        assert!(!DiagnosticSessionControl::supports(
            &ecu,
            DiagnosticSessionType::SafetySystemDiagnosticSession
        ));
    }

    /// P2 values are a property of the session being entered, so the application states
    /// them and this crate composes the response; the session layer reads them in
    /// milliseconds, `P2*` being sent in tens of them.
    #[test]
    fn a_session_carries_its_own_timing() {
        let t = Ecu.timing(DiagnosticSessionType::ProgrammingSession);
        assert_eq!((t.p2_server_max(), t.p2_star_server_max()), (50, 5_000));
        let widest = SessionTiming {
            p2_server_max_ms: u16::MAX,
            p2_star_server_max_10ms: u16::MAX,
        };
        assert_eq!(widest.p2_star_server_max(), 655_350);
    }
}
