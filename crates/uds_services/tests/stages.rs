//! Each service's own stage, through the `dispatch` that `uds_server!` emits and the
//! `settle` the driver calls, with no transport: one test per clause check, and one
//! positive exchange per service.

#![allow(
    clippy::unused_async_trait_impl,
    reason = "the fixture's handlers answer at once; they are async because the traits are"
)]

use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::pipeline::settle;
use uds_services::{
    Address, Ai, DiagnosticSessionType as S, EcuReset, Mtype, ProtocolState, ResetType,
    Responded, ResponseSink, ServiceSet, SessionTiming, SessionTransition, Sink, TaType,
    TesterPresent, uds_server,
};

#[derive(Debug, Default)]
struct Ecu {
    /// The reset `EcuReset::reset` last accepted.
    accepted: Option<ResetType>,
    /// Makes the next reset fail its criteria.
    refuse_reset: bool,
}

impl uds_services::DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
    }
    fn supported_from(&self, _s: S, _active: S) -> bool {
        true
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _relocked: bool) {}
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn supports(&self, kind: ResetType) -> bool {
        matches!(
            kind,
            ResetType::HardReset
                | ResetType::KeyOffOnReset
                | ResetType::SoftReset
                | ResetType::EnableRapidPowerShutDown
        )
    }
    /// A key-off-on reset is offered only in the extended session.
    fn supported_in(&self, kind: ResetType, active: S) -> bool {
        !matches!(kind, ResetType::KeyOffOnReset)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    async fn reset(
        &mut self,
        kind: ResetType,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        if core::mem::take(&mut self.refuse_reset) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        if matches!(kind, ResetType::EnableRapidPowerShutDown) {
            let _ = out.write_all(&[0x0A]);
        }
        self.accepted = Some(kind);
        Ok(())
    }
}

#[derive(Debug)]
struct NoTransport;

impl uds_services::UdsTransport for NoTransport {
    type Error = ();
    async fn t_data_req(&mut self, _ai: Ai, _d: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    async fn next_event<'b>(
        &mut self,
        _b: &'b mut [u8],
        _d: Option<uds_services::Timestamp>,
    ) -> Result<uds_services::TransportEvent<'b>, ()> {
        Ok(uds_services::TransportEvent::Deadline)
    }
    fn outbound_max(&self) -> Option<usize> {
        None
    }
    fn channel_timing(&self) -> uds_services::Reloads {
        uds_services::Reloads {
            default_reload: 2_000,
            enhanced_reload: 5_000,
        }
    }
    fn now(&self) -> uds_services::Timestamp {
        uds_services::Timestamp(0)
    }
}

uds_server! {
    Ecu: DiagnosticSessionControl, TesterPresent, EcuReset;
    transport = NoTransport,
    peers = 1,
    server = EcuServer,
}

type State = <Ecu as ServiceSet>::State;

const PHYSICAL: Ai = Ai {
    mtype: Mtype::Diag,
    sa: Address(0x0E80),
    ta: Address(0x0010),
    ta_type: TaType::Physical,
};

#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
fn block_on<F: core::future::Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("the fixture's handlers never pend"),
    }
}

/// What one physically addressed request produced: the bytes written, or `None` where
/// the response was suppressed.
fn exchange(ecu: &mut Ecu, state: &mut State, request: &[u8]) -> Option<Vec<u8>> {
    let mut buf = [0_u8; 64];
    let mut out = ResponseSink::new(&mut buf, None);
    let unsettled = block_on(ecu.dispatch(state, PHYSICAL, request, &mut out));
    match settle(PHYSICAL, unsettled, false, &mut out) {
        Responded::Yes { .. } => Some(out.written_bytes().to_vec()),
        Responded::Suppressed { .. } => None,
    }
}

fn in_session(ecu: &mut Ecu, session: S) -> State {
    let mut state = State::INITIAL;
    ecu.session_confirmed(&mut state, session);
    state
}

// --- EcuReset (0x11), ISO 14229-1:2020 clause 10.3 ------------------------------------

/// ``UDSSVC_ARCH_0007`` row 2, clause 10.3.4 — a reset the server does not support, a
/// reserved `resetType`, and either with a trailing byte are 0x12, not 0x13, and the
/// handler is not asked.
#[test]
fn ecu_reset_an_unsupported_reset_type_is_0x12() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x11, 0x05][..],
        &[0x11, 0x00][..],
        &[0x11, 0x7F][..],
        &[0x11, 0x85][..],
        &[0x11, 0x05, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x11, 0x12][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.accepted, None);
}

/// ``UDSSVC_ARCH_0007`` row 4 — a reset supported, but not in the active session, is
/// 0x7E; the same request proceeds from the session that offers it.
#[test]
fn ecu_reset_a_reset_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x02]).as_deref(),
        Some(&[0x7F, 0x11, 0x7E][..])
    );
    let mut state = in_session(&mut ecu, S::ExtendedDiagnosticSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x02]).as_deref(),
        Some(&[0x51, 0x02][..])
    );
}

/// Clause 10.3.2.3 — the request carries no data-parameters, so a supported reset with a
/// trailing byte is 0x13, and the handler is not asked.
#[test]
fn ecu_reset_a_trailing_byte_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x01, 0x00]).as_deref(),
        Some(&[0x7F, 0x11, 0x13][..])
    );
    assert_eq!(ecu.accepted, None);
}

/// Clause 10.3.4 — the handler's `conditionsNotCorrect` (0x22) replaces the response the
/// stage had begun.
#[test]
fn ecu_reset_the_handler_refusal_is_its_code() {
    let mut ecu = Ecu {
        refuse_reset: true,
        ..Ecu::default()
    };
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x01]).as_deref(),
        Some(&[0x7F, 0x11, 0x22][..])
    );
    assert_eq!(ecu.accepted, None);
}

/// Clause 10.3.3, Tables 35 and 39 — the positive response echoes `resetType`; only
/// `enableRapidPowerShutDown` carries the `powerDownTime` the handler wrote.
#[test]
fn ecu_reset_the_positive_response_echoes_the_reset_type() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x01]).as_deref(),
        Some(&[0x51, 0x01][..])
    );
    assert_eq!(ecu.accepted, Some(ResetType::HardReset));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x04]).as_deref(),
        Some(&[0x51, 0x04, 0x0A][..])
    );
}

/// ``UDSSVC_ARCH_0009`` rule 2 — with the suppress bit the reset is still accepted, and
/// no response is sent.
#[test]
fn ecu_reset_the_suppress_bit_silences_the_positive_response() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(exchange(&mut ecu, &mut state, &[0x11, 0x83]), None);
    assert_eq!(ecu.accepted, Some(ResetType::SoftReset));
}
