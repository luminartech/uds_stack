//! Routine — ISO 14229-1:2020 clause 13.

use crate::{ResponseSink, RoutineIdentifier};
use uds_protocol::NegativeResponseCode;

/// `RoutineControl` (0x31).
///
/// ``UDSSVC_ARCH_0007`` — Figure 5 excludes service identifier 0x31 from the centralised
/// sub-function stage, because a routine's sub-function is meaningful only together with
/// its routine identifier: whether `stopRoutine` is supported is a property of the
/// routine, not of the service. **Three methods rather than one with a sub-function
/// parameter**, so that asymmetry is visible in the type rather than documented. Settles
/// open question 5.
pub trait RoutineControl {
    /// This application's routine identifier enumeration.
    type Rid: RoutineIdentifier;

    /// ``UDSSVC_ARCH_0033`` — a routine outrunning `tP2_Server` is the paradigm case.
    const MAY_RESPOND_PENDING: bool;

    /// The longest option record this server accepts.
    const MAX_OPTION_LEN: usize;

    /// `startRoutine` (0x01).
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`], including `subFunctionNotSupported` (0x12) where
    /// this routine cannot be started — a decision no other service's handler makes.
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
    /// The [`NegativeResponseCode`], including 0x12 where this routine cannot be stopped.
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
    /// The [`NegativeResponseCode`], including 0x12 where this routine reports none.
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
    /// not of the service. Three separate methods make that asymmetry structural: a
    /// routine that does not support stopping returns 0x12 from `stop` alone. Settles
    /// open question 5.
    ///
    /// This is a trait-bound check and nothing more: it establishes that `start`, `stop`
    /// and `results` are each required, because the fixture above satisfies
    /// `RoutineControl` only by implementing all three. No 0x12 behaviour is observed
    /// here — there is none to observe until the pipeline lands.
    #[test]
    fn routine_control_requires_start_stop_and_results_separately() {
        fn assert_three_methods<T: RoutineControl>() {}
        assert_three_methods::<Ecu>();
    }
}
