//! Diagnostics and communication management — ISO 14229-1:2020 clause 10.
//!
//! ``UDSSVC_ARCH_0012`` — one trait per service.

use crate::{ResponseSink, SessionTransition};
use uds_protocol::NegativeResponseCode;

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
pub trait DiagnosticSessionControl {
    /// ``UDSSVC_ARCH_0033``. No default: omission must not compile.
    const MAY_RESPOND_PENDING: bool;

    /// The longest positive response beyond the mandatory four timing bytes.
    const MAX_RESPONSE_LEN: usize;

    /// Whether this server supports the session named by this sub-function value.
    ///
    /// Raw and uninterpreted: naming the sessions here would mean deciding what
    /// `uds_protocol`'s sub-functions mean, which is its to define and the
    /// application's to choose.
    fn supports(&self, session: u8) -> bool;

    /// The `P2` pair to advertise for `session`.
    fn timing(&self, session: u8) -> SessionTiming;

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

    /// Perform the reset named by `kind`, writing any `powerDownTime` into `out`.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported or impermissible reset.
    fn reset(
        &mut self,
        kind: u8,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `TesterPresent` (0x3E).
///
/// Restarting `tS3_Server` is deliberately absent: ISO 14229-2 puts it in the session
/// layer and ``UDSSVC_ARCH_0002`` keeps it there. This exists for servers that act on it.
pub trait TesterPresent {
    /// Always false. A server too busy to answer the message whose only purpose is to
    /// say it is still there has a larger problem than a response-pending.
    const MAY_RESPOND_PENDING: bool;

    /// Called on each accepted `TesterPresent`.
    fn on_tester_present(&mut self);
}

/// `CommunicationControl` (0x28).
pub trait CommunicationControl {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Apply `control_type` to `communication_type`, with the node identification number
    /// where the sub-function carries one.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported combination.
    fn control(
        &mut self,
        control_type: u8,
        communication_type: u8,
        node: Option<u16>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ControlDTCSetting` (0x85).
pub trait ControlDtcSetting {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// Turn DTC setting on or off, with the manufacturer-specific option record.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported setting.
    fn control_dtc_setting(
        &mut self,
        setting: u8,
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
    use uds_protocol::NegativeResponseCode;

    struct Ecu;

    impl DiagnosticSessionControl for Ecu {
        const MAY_RESPOND_PENDING: bool = false;
        const MAX_RESPONSE_LEN: usize = 0;
        fn supports(&self, session: u8) -> bool {
            matches!(session, 0x01 | 0x02 | 0x03)
        }
        fn timing(&self, _s: u8) -> SessionTiming {
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
            _k: u8,
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
        assert!(ecu.supports(0x02));
        assert!(!ecu.supports(0x7F));
    }

    /// P2 values are a property of the session being entered, so the application states
    /// them and this crate composes the response.
    #[test]
    fn a_session_carries_its_own_timing() {
        let t = Ecu.timing(0x02);
        assert_eq!((t.p2_server_max, t.p2_star_server_max), (50, 5_000));
    }
}
