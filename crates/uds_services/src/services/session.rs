//! Diagnostics and communication management, ISO 14229-1:2020 clause 10.
//!
//! ``UDSSVC_ARCH_0012``: one trait per service.

use crate::{Access, SessionTransition};
use uds_protocol::{
    CommunicationControlType, CommunicationType, DiagnosticSessionType, DtcSettingType,
    NegativeResponseCode, P2StarServerMax, ResetType, SubnetNumber,
};

/// ``UDSS_LLR_0073``: the session layer learns only default/non-default.
pub(crate) fn selection_of(
    session: DiagnosticSessionType,
) -> uds_session::SessionSelection {
    if matches!(session, DiagnosticSessionType::DefaultSession) {
        uds_session::SessionSelection::Default
    } else {
        uds_session::SessionSelection::NonDefault
    }
}

/// The `P2` pair a session advertises in its `DiagnosticSessionControl` response, and
/// that [`crate::Server`] enforces while the session is in force.
///
/// ISO 14229-1:2020 clause 10.2: the positive response carries `P2Server_max` and
/// `P2*Server_max` for the session being entered. Held in Table 29's wire form, so the
/// window enforced is the one advertised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionTiming {
    /// `P2Server_max`, in 1 ms units.
    pub p2_server_max_ms: u16,
    /// `P2*Server_max`; see [`P2StarServerMax`].
    pub p2_star_server_max: P2StarServerMax,
}

impl SessionTiming {
    /// `P2Server_max` in milliseconds, the session layer's unit.
    pub(crate) fn p2_server_max(self) -> u32 {
        u32::from(self.p2_server_max_ms)
    }

    /// `P2*Server_max` in milliseconds, the session layer's unit.
    pub(crate) fn p2_star_server_max(self) -> u32 {
        self.p2_star_server_max.as_millis()
    }
}

/// `DiagnosticSessionControl` (0x10).
///
/// ``UDSSVC_ARCH_0035`` puts the active session in this crate and ``UDSSVC_ARCH_0038``
/// classifies the change before the application sees it, so an application never tracks
/// a session itself.
///
/// [`Self::prepare`] is the stage the driver awaits (``UDSSVC_ARCH_0046``), so the
/// service declares [`Self::MAY_RESPOND_PENDING`] as every service in
/// ``UDSSVC_ARCH_0033`` does.
pub trait DiagnosticSessionControl {
    /// ``UDSSVC_ARCH_0033``: whether the driver may answer
    /// `requestCorrectlyReceivedResponsePending` (0x78) while [`Self::prepare`] runs.
    const MAY_RESPOND_PENDING: bool;

    /// The longest positive response beyond the mandatory four timing bytes.
    const MAX_RESPONSE_LEN: usize;

    /// Whether this server supports `session` at all, from whichever session.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, "`SubFunction` supported ever for the SID?",
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
    /// for the SID?", ``UDSSVC_ARCH_0007`` row 4. Asked only for a `session` that
    /// [`Self::supports`] accepted, so a `false` settles the request
    /// `subFunctionNotSupportedInActiveSession` (0x7E), which Annex A reserves for a
    /// sub-function "known to be supported in another session". A server that restricts
    /// no transition returns `true`.
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
    /// session in force each time a request arrives, when [`crate::Server`] loads the pair
    /// into the session layer. Every session the server is in, the default one included,
    /// is timed by this and not by the [`ServerParams`](crate::ServerParams) passed to
    /// [`Server::new`](crate::Server::new). The returned pair must keep those params'
    /// `response_pending_lead` well formed
    /// ([`ServerParams::lead_fits`](crate::ServerParams::lead_fits)); the server panics
    /// on the first request timed by a pair that does not.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being entered; see [`DiagnosticSessionType`].
    fn timing(&self, session: DiagnosticSessionType) -> SessionTiming;

    /// Whether entering `session` leaves the software this server is running.
    ///
    /// ISO 14229-1:2020 10.2.2.2 Table 25 lets the programming session run in boot
    /// software, so each binary says: an application jumping to its bootloader answers
    /// `true` for [`ProgrammingSession`](DiagnosticSessionType::ProgrammingSession), the
    /// bootloader `true` for [`DefaultSession`](DiagnosticSessionType::DefaultSession)
    /// where valid application software exists, and a server that programs from within
    /// its application `false` for every session. [`crate::Server`] hands the final
    /// positive response [`AfterSend::ServerLeaves`](crate::AfterSend::ServerLeaves), on
    /// which a connection-oriented transport closes (ISO 14229-5:2022 REQ 7.9).
    ///
    /// A suppressed session change sends no response, so no orderly close precedes the
    /// departure: a server left by `10 82` drops its connection unannounced. A client
    /// that needs the close must not suppress the response.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being entered; see [`DiagnosticSessionType`].
    fn leaves_running_software(&self, session: DiagnosticSessionType) -> bool;

    /// Reach the state `session` needs, or refuse it, before the positive response
    /// (``UDSSVC_ARCH_0046``): stop what the server is doing before a programming
    /// session, say. Asked once [`Self::supports`] and [`Self::supported_from`] have
    /// admitted the request, so what remains is whether the server's own state allows the
    /// change now.
    ///
    /// `session` is entered, and [`Self::on_transition`] called, only once the positive
    /// response is confirmed sent, or a suppressed one's completion reported. Where the
    /// change accepted is not made, [`Self::abandon`] is called instead.
    ///
    /// With [`Self::MAY_RESPOND_PENDING`] the driver answers 0x78 each time the response
    /// window runs out first. A client may give up after a ceiling of its own on those,
    /// such as [`ClientTiming::max_response_pending`], so a `prepare` that can pend long
    /// completes within what its clients accept.
    ///
    /// # Arguments
    ///
    /// * `session` - the session being requested; see [`DiagnosticSessionType`].
    /// * `active` - the session the server is in when the request arrives.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] refusing the change, which nothing is entered after.
    /// It is sent as returned, under the suppression rules every handler's code is
    /// (``UDSSVC_ARCH_0009``): `conditionsNotCorrect` (0x22) where the server's state
    /// does not allow the change, ISO 14229-1:2020 clause 10.2.4.
    ///
    /// # Cancel safety
    ///
    /// The future is dropped unfinished where the requesting client's link closes, where
    /// the transport fails and [`Server::step`](crate::Server::step) returns a
    /// [`StepError::Terminal`](crate::StepError::Terminal), and where `step` is dropped.
    /// Nothing is sent, the session does not change and [`Self::abandon`] is not called,
    /// so every await must leave a state valid in the active session.
    ///
    /// [`ClientTiming::max_response_pending`]: crate::ClientTiming::max_response_pending
    fn prepare(
        &mut self,
        session: DiagnosticSessionType,
        active: DiagnosticSessionType,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// The change [`Self::prepare`] accepted is not made, and the server stays in
    /// `active`: undo what `prepare` changed. By default, nothing is done.
    ///
    /// Called in place of entering `session` where the positive response is refused after
    /// all (`responseTooLong`, say), its confirmation fails, the requesting client's link
    /// closes before it, `tS3_Server` expires first, the transport fails mid-request
    /// before it ([`StepError::Terminal`](crate::StepError::Terminal)), or
    /// [`Server::step`](crate::Server::step) is dropped before the session is entered. A
    /// transport failing between requests
    /// ([`StepError::Recoverable`](crate::StepError::Recoverable)) abandons nothing: the
    /// confirmation the next step takes enters the session.
    ///
    /// # Arguments
    ///
    /// * `session` - the session `prepare` accepted; see [`DiagnosticSessionType`].
    /// * `active` - the session the server is in.
    fn abandon(&mut self, _session: DiagnosticSessionType, _active: DiagnosticSessionType) {
    }

    /// Called once the positive response is confirmed sent, or a suppressed one's
    /// completion reported, and on `tS3_Server` expiry.
    ///
    /// ``UDSSVC_ARCH_0038``. The application applies what clause 10.2 Figure 7 owes the
    /// state it holds: `transition` says which of the four cases occurred, and `entered`
    /// which session, since `ControlDTCSetting` and `CommunicationControl` state resumes
    /// on entering a session where the service is not supported.
    ///
    /// # Arguments
    ///
    /// * `transition` - which of Figure 7's transitions occurred; see
    ///   [`SessionTransition`].
    /// * `entered` - the session now in force; see [`DiagnosticSessionType`].
    ///   [`DefaultSession`](DiagnosticSessionType::DefaultSession) on `tS3_Server` expiry.
    /// * `security_relocked` - whether the transition relocked a security level, so
    ///   functionality gated on it can be dropped: Annex I transition 6 makes a session
    ///   change lock every level.
    fn on_transition(
        &mut self,
        transition: SessionTransition,
        entered: DiagnosticSessionType,
        security_relocked: bool,
    );
}

/// What an accepted reset does once its response is sent (``UDSSVC_ARCH_0045``).
///
/// The application says, per request: the crate holds no table of which reset types
/// restart, because the manufacturer- and supplier-specific ones have no standard answer.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AfterReset {
    /// The server restarts: `hardReset`, `keyOffOnReset`, `softReset`, and any specific
    /// reset that restarts. Once the response is confirmed the protocol state returns to
    /// the power-up state and [`Server::step`](crate::Server::step) reports
    /// [`Stopped::Reset`](crate::Stopped::Reset).
    Restart,
    /// The server keeps serving: `enableRapidPowerShutDown`,
    /// `disableRapidPowerShutDown`.
    Continue,
}

/// `EcuReset` (0x11).
///
/// ISO 14229-1:2020 clause 10.3. The pipeline writes the positive response's service
/// identifier and echoed `resetType`; [`Self::reset`] decides whether the reset is
/// accepted and supplies the `powerDownTime` that follows.
pub trait EcuReset {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Where `kind` is available and which levels unlock it, or `None` where this
    /// server does not support it.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, ``UDSSVC_ARCH_0007``: in that order, `None`
    /// settles the request `subFunctionNotSupported` (0x12), an active session outside
    /// [`Access::sessions`] `subFunctionNotSupportedInActiveSession` (0x7E), and none of
    /// [`Access::levels`] unlocked `securityAccessDenied` (0x33), each without
    /// [`Self::reset`] being asked.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested; see [`ResetType`], whose reserved and specific
    ///   variants carry the raw byte for a server that defines its own.
    fn access(&self, kind: ResetType) -> Option<Access>;

    /// Accept or refuse `kind`, and say what follows the response: [`AfterReset`].
    ///
    /// **Must not perform the reset before returning.** ISO 14229-1:2020 clause 10.3.1
    /// strongly recommends that the positive response is sent before the reset is
    /// executed, and it is sent only after this returns `Ok`.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset requested, one [`Self::access`] admitted; see [`ResetType`].
    /// * `active` - the session the server is in when the request arrives.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a reset whose criteria are not met, such as
    /// `conditionsNotCorrect` (0x22), clause 10.3.4. A locked level is
    /// [`Self::access`]'s, not this method's.
    fn reset(
        &mut self,
        kind: ResetType,
        active: DiagnosticSessionType,
    ) -> impl core::future::Future<Output = Result<AfterReset, NegativeResponseCode>>;

    /// The `powerDownTime` an accepted [`ResetType::EnableRapidPowerShutDown`]'s
    /// response carries, the one reset whose response does (ISO 14229-1:2020 Table 35):
    /// seconds from `0x00` to `0xFE`, or `0xFF` for a failure or a time not available,
    /// which is what this returns unless overridden. Asked after [`Self::reset`] accepts
    /// that reset, and for no other.
    fn power_down_time(&self) -> u8 {
        0xFF
    }

    /// The positive response to `kind` was confirmed sent, or its suppression completed
    /// (``UDSS_LLR_0085``/``0086``). Where a reset starts something after its response,
    /// such as the rapid power shutdown ISO 14229-1:2020 Table 34 starts only after the
    /// positive response, it starts here.
    ///
    /// Called for every accepted reset, before the protocol state returns to the power-up
    /// state for an [`AfterReset::Restart`]. Where the reset is not made,
    /// [`Self::abandon`] is called instead.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset whose response was confirmed; see [`ResetType`].
    fn confirmed(&mut self, kind: ResetType);

    /// The reset [`Self::reset`] accepted is not made: undo what `reset` changed. By
    /// default, nothing is done.
    ///
    /// Called in place of [`Self::confirmed`] where, as for
    /// [`DiagnosticSessionControl::abandon`], the positive response is refused after
    /// all, its confirmation fails, the requesting client's link closes before it, the
    /// transport fails mid-request before it, or [`Server::step`](crate::Server::step) is
    /// dropped first. Not called for a `reset` dropped unfinished.
    ///
    /// # Arguments
    ///
    /// * `kind` - the reset `reset` accepted; see [`ResetType`].
    /// * `active` - the session the server is in.
    fn abandon(&mut self, _kind: ResetType, _active: DiagnosticSessionType) {}
}

/// `TesterPresent` (0x3E), for servers that act on it. Restarting `tS3_Server` is the
/// session layer's (ISO 14229-2, ``UDSSVC_ARCH_0002``), not this trait's.
///
/// No `MAY_RESPOND_PENDING`: [`Self::on_tester_present`] is synchronous, so no 0x78 can
/// come due. No sub-function lookups either: the service's only sub-function is
/// `zeroSubFunction`, which ISO 14229-1:2020 clause 10.7 makes mandatory and Table 23
/// makes available in every session, so the pipeline answers ``UDSSVC_ARCH_0007`` row 2
/// from the byte and row 4 is always yes.
pub trait TesterPresent {
    /// Called on each accepted `TesterPresent`.
    fn on_tester_present(&mut self);
}

/// `CommunicationControl` (0x28).
pub trait CommunicationControl {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Where `control_type` is available and which levels unlock it, or `None` where this
    /// server does not support it.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, ``UDSSVC_ARCH_0007``: in that order, `None`
    /// settles the request `subFunctionNotSupported` (0x12), an active session outside
    /// [`Access::sessions`] `subFunctionNotSupportedInActiveSession` (0x7E), and none of
    /// [`Access::levels`] unlocked `securityAccessDenied` (0x33), each without
    /// [`Self::control`] being asked. The default session never reaches here: Table 23
    /// refuses the service there with 0x7F.
    ///
    /// # Arguments
    ///
    /// * `control_type` - the `controlType` requested; see [`CommunicationControlType`],
    ///   whose reserved and specific variants carry the raw byte.
    fn access(&self, control_type: CommunicationControlType) -> Option<Access>;

    /// Apply `control_type` to `communication_type` on `subnet`, or on the node `node_id`
    /// names.
    ///
    /// Clause 10.5.1: the positive response is owed even where the requested state is
    /// already in effect. Clause 10.2 Figure 7 resets that state on a return to the
    /// default session, which the application does when
    /// `DiagnosticSessionControl::on_transition` tells it the session entered.
    ///
    /// # Arguments
    ///
    /// * `control_type` - what to do; see [`CommunicationControlType`].
    /// * `communication_type` - what it applies to; see [`CommunicationType`]. Never
    ///   [`CommunicationType::IsoSaeReserved`]: Annex B Table B.1 reserves those values,
    ///   and the pipeline answers them `requestOutOfRange` (0x31) without asking.
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

    /// The longest `DTCSettingControlOptionRecord` this server accepts, folded into the
    /// in-flight buffer; clause 10.8.2.3 gives the record no fixed width. A longer record
    /// is `incorrectMessageLengthOrInvalidFormat` (0x13) without
    /// [`Self::control_dtc_setting`] being asked.
    const MAX_OPTION_RECORD_LEN: usize;

    /// Where `setting` is available and which levels unlock it, or `None` where this
    /// server does not support it.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, ``UDSSVC_ARCH_0007``: in that order, `None`
    /// settles the request `subFunctionNotSupported` (0x12), an active session outside
    /// [`Access::sessions`] `subFunctionNotSupportedInActiveSession` (0x7E), and none of
    /// [`Access::levels`] unlocked `securityAccessDenied` (0x33), each without
    /// [`Self::control_dtc_setting`] being asked. The default session never reaches
    /// here: Table 23 refuses the service there with 0x7F.
    ///
    /// # Arguments
    ///
    /// * `setting` - the `DTCSettingType` requested; see [`DtcSettingType`]. A reserved
    ///   `DTCSettingType` is not one, and is 0x12 without this being asked.
    fn access(&self, setting: DtcSettingType) -> Option<Access>;

    /// Apply `setting`, with the manufacturer-specific option record.
    ///
    /// Clause 10.8.1: the positive response is owed even where `setting` is already in
    /// effect, and updating resumes on a transition to a session where this service is
    /// not supported, which the application does when
    /// `DiagnosticSessionControl::on_transition` tells it the session entered.
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
    use super::{AfterReset, DiagnosticSessionControl, EcuReset, SessionTiming};
    use crate::SessionTransition;
    use uds_protocol::{DiagnosticSessionType, NegativeResponseCode, ResetType};

    struct Ecu;

    impl DiagnosticSessionControl for Ecu {
        const MAY_RESPOND_PENDING: bool = false;
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
        fn leaves_running_software(&self, _s: DiagnosticSessionType) -> bool {
            false
        }
        fn timing(&self, _s: DiagnosticSessionType) -> SessionTiming {
            SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max: crate::P2StarServerMax::from_tens_of_millis(500),
            }
        }
        fn prepare(
            &mut self,
            _s: DiagnosticSessionType,
            _active: DiagnosticSessionType,
        ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>> {
            core::future::ready(Ok(()))
        }
        fn on_transition(
            &mut self,
            _t: SessionTransition,
            _entered: DiagnosticSessionType,
            _relocked: bool,
        ) {
        }
    }

    impl EcuReset for Ecu {
        const MAY_RESPOND_PENDING: bool = true;
        fn access(&self, _k: ResetType) -> Option<crate::Access> {
            Some(crate::Access::new(crate::Sessions::ALL))
        }
        async fn reset(
            &mut self,
            _k: ResetType,
            _active: DiagnosticSessionType,
        ) -> Result<AfterReset, NegativeResponseCode> {
            Ok(AfterReset::Restart)
        }
        fn confirmed(&mut self, _k: ResetType) {}
    }

    /// ``UDSSVC_ARCH_0038``: the application receives a classified transition, not a
    /// sub-function value.
    #[test]
    fn the_application_receives_a_classified_transition() {
        let mut ecu = Ecu;
        ecu.on_transition(
            SessionTransition::NonDefaultToDefault,
            DiagnosticSessionType::DefaultSession,
            true,
        );
        assert!(DiagnosticSessionControl::supports(
            &ecu,
            DiagnosticSessionType::ProgrammingSession
        ));
        assert!(!DiagnosticSessionControl::supports(
            &ecu,
            DiagnosticSessionType::SafetySystemDiagnosticSession
        ));
    }

    /// The application states a session's P2 pair in Table 29's units, and the session
    /// layer reads both in milliseconds.
    #[test]
    fn a_session_carries_its_own_timing() {
        let t = Ecu.timing(DiagnosticSessionType::ProgrammingSession);
        assert_eq!((t.p2_server_max(), t.p2_star_server_max()), (50, 5_000));
        let widest = SessionTiming {
            p2_server_max_ms: u16::MAX,
            p2_star_server_max: crate::P2StarServerMax::from_tens_of_millis(u16::MAX),
        };
        assert_eq!(widest.p2_star_server_max(), 655_350);
    }
}
