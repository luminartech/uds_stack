//! Routine — ISO 14229-1:2020 clause 14.

use crate::{ResponseSink, RoutineIdentifier, SecurityLevel};
use uds_protocol::{
    DiagnosticSessionType, NegativeResponseCode, RoutineControlSubFunction,
};

/// `RoutineControl` (0x31).
///
/// ``UDSSVC_ARCH_0007`` — Figure 5 excludes service identifier 0x31 from the centralised
/// sub-function stage, because a routine's sub-function is meaningful only together with
/// its routine identifier: whether `stopRoutine` is supported is a property of the
/// routine, not of the service. **Three methods rather than one with a sub-function
/// parameter**, so that asymmetry is visible in the type rather than documented. Settles
/// open question 5. Whether a routine supports a sub-function at all is nonetheless also a
/// lookup, [`Self::supports`], because Figure 30 answers it before the option record's
/// length is checked, and the pipeline must not hand a method a record longer than
/// [`Self::MAX_OPTION_LEN`].
///
/// ISO 14229-1:2020 clause 14.2, Figure 30 — the pipeline settles, in order: a request
/// shorter than its routine identifier (0x13), an identifier
/// [`RoutineIdentifier::from_u16`] rejects or [`Self::supported_in`] refuses (0x31), a
/// locked [`Self::required_level`] (0x33), a `routineControlType` Table 426 reserves or
/// [`Self::supports`] refuses for this routine (0x12), and an option record longer than
/// [`Self::MAX_OPTION_LEN`] (0x13). Only then is the sub-function's method asked, and
/// Figure 30's remaining checks are its own: the record's length for this routine (0x13),
/// conditions (0x22), the record's content (0x31) and the request sequence (0x24). Each
/// method writes `routineInfo` and any `routineStatusRecord` into its `out`, after the
/// `71`, the echoed `routineControlType` and the identifier the pipeline wrote (Table
/// 428).
pub trait RoutineControl {
    /// This application's routine identifier enumeration.
    type Rid: RoutineIdentifier;

    /// ``UDSSVC_ARCH_0033`` — a routine outrunning `tP2_Server` is the paradigm case.
    const MAY_RESPOND_PENDING: bool;

    /// The longest option record this server accepts; a longer one is
    /// `incorrectMessageLengthOrInvalidFormat` (0x13) without a handler being asked.
    const MAX_OPTION_LEN: usize;

    /// Whether `routine` is available in the `active` session.
    ///
    /// Figure 30, "RID supported in active session?" — a `false` settles the request
    /// `requestOutOfRange` (0x31), as an identifier this application does not define is.
    ///
    /// # Arguments
    ///
    /// * `routine` - the identifier the request names; see [`Self::Rid`].
    /// * `active` - the session the server is in when the request arrives; see
    ///   [`DiagnosticSessionType`].
    fn supported_in(&self, routine: Self::Rid, active: DiagnosticSessionType) -> bool;

    /// The security level `routine` requires unlocked, or `None` where it requires none.
    ///
    /// Figure 30, "RID security check OK?" — where the level this crate holds unlocked
    /// is not the one returned, the request settles `securityAccessDenied` (0x33) without
    /// a handler being asked.
    ///
    /// # Arguments
    ///
    /// * `routine` - the identifier the request names; see [`Self::Rid`].
    fn required_level(&self, routine: Self::Rid) -> Option<SecurityLevel>;

    /// Whether `routine` supports `control` at all.
    ///
    /// Figure 30, "`SubFunction` supported for `routineIdentifier`?" — asked only for a
    /// `routine` [`Self::supported_in`] accepted and whose [`Self::required_level`] is
    /// unlocked, and only for `startRoutine`, `stopRoutine` and `requestRoutineResults`:
    /// a reserved `routineControlType` is 0x12 without being asked. A `false` settles the
    /// request `subFunctionNotSupported` (0x12) before the option record's length is
    /// checked and without the matching method being asked.
    ///
    /// # Arguments
    ///
    /// * `routine` - the identifier the request names; see [`Self::Rid`].
    /// * `control` - the `routineControlType` requested; see [`RoutineControlSubFunction`].
    fn supports(&self, routine: Self::Rid, control: RoutineControlSubFunction) -> bool;

    /// `startRoutine` (0x01).
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a start [`Self::supports`] accepted but that
    /// cannot be made: 0x13, 0x22, 0x24, 0x31 or 0x72 (clause 14.2.4).
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn start(
        &mut self,
        routine: Self::Rid,
        option_record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// `stopRoutine` (0x02).
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a stop that cannot be made, as for [`Self::start`].
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn stop(
        &mut self,
        routine: Self::Rid,
        option_record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;

    /// `requestRoutineResults` (0x03).
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where results cannot be given, as for [`Self::start`]:
    /// `requestSequenceError` (0x24) where the routine has produced none.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn results(
        &mut self,
        routine: Self::Rid,
        option_record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the fixtures never await — that is not a real defect in test code"
)]
mod tests {
    use super::RoutineControl;
    use crate::{ResponseSink, RoutineIdentifier};
    use uds_protocol::NegativeResponseCode;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Rid(u16);
    impl RoutineIdentifier for Rid {
        const MAX_STATUS_LEN: usize = 8;
        fn as_u16(self) -> u16 {
            self.0
        }
        fn from_u16(v: u16) -> Option<Self> {
            Some(Self(v))
        }
    }

    struct Ecu;
    impl RoutineControl for Ecu {
        type Rid = Rid;
        const MAY_RESPOND_PENDING: bool = true;
        const MAX_OPTION_LEN: usize = 4;
        fn supported_in(&self, _r: Rid, _a: uds_protocol::DiagnosticSessionType) -> bool {
            true
        }
        fn required_level(&self, _r: Rid) -> Option<crate::SecurityLevel> {
            None
        }
        fn supports(
            &self,
            _r: Rid,
            control: uds_protocol::RoutineControlSubFunction,
        ) -> bool {
            !matches!(
                control,
                uds_protocol::RoutineControlSubFunction::StopRoutine
            )
        }
        async fn start(
            &mut self,
            _r: Rid,
            _o: &[u8],
            _s: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
        async fn stop(
            &mut self,
            _r: Rid,
            _o: &[u8],
            _s: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Err(NegativeResponseCode::SubFunctionNotSupported)
        }
        async fn results(
            &mut self,
            _r: Rid,
            _o: &[u8],
            _s: &mut ResponseSink<'_>,
        ) -> Result<(), NegativeResponseCode> {
            Ok(())
        }
    }

    /// ``UDSSVC_ARCH_0007`` — Figure 5 excludes 0x31 from the centralised sub-function
    /// stage, because whether `stopRoutine` is supported is a property of the *routine*,
    /// not of the service. Three separate methods make that asymmetry structural, and
    /// `supports` answers 0x12 for the pair. Settles open question 5.
    ///
    /// This is a trait-bound check and nothing more: it establishes that `start`, `stop`
    /// and `results` are each required, because the fixture above satisfies
    /// `RoutineControl` only by implementing all three. No 0x12 behaviour is observed
    /// here; `tests/stages.rs` observes it through the pipeline.
    #[test]
    fn routine_control_requires_start_stop_and_results_separately() {
        fn assert_three_methods<T: RoutineControl>() {}
        assert_three_methods::<Ecu>();
    }
}
