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
    Address, Ai, DiagnosticSessionType as S, EcuReset, KeyVerdict, Mtype, ProtocolState,
    ResetType, Responded, ResponseSink, SecurityAccess, SecurityLevel, SecurityPolicy,
    ServiceSet, SessionTiming, SessionTransition, Sink, TaType, TesterPresent, uds_server,
};

#[derive(Debug, Default)]
struct Ecu {
    /// The reset `EcuReset::reset` last accepted.
    accepted: Option<ResetType>,
    /// Makes the next reset fail its criteria.
    refuse_reset: bool,
    /// Level 0x01's stored attempt count.
    attempts: u8,
    /// Whether level 0x01's delay is running.
    delay: bool,
    /// How many times a delay was started.
    delays_started: u8,
    /// The `security_relocked` of the last session transition.
    relocked: Option<bool>,
}

impl uds_services::DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
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
    fn on_transition(&mut self, _t: SessionTransition, relocked: bool) {
        self.relocked = Some(relocked);
    }
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

/// Level 0x01 counts attempts, three before a delay; level 0x03 counts none; level 0x05
/// is offered only in the programming session. Each seed is fixed, and its key is the
/// seed's two's complement (clause 10.4.5.1).
impl SecurityAccess for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_SEED_LEN: usize = 2;
    const MAX_KEY_LEN: usize = 2;
    fn supports(&self, level: SecurityLevel) -> bool {
        matches!(level.request_seed(), 0x01 | 0x03 | 0x05)
    }
    fn supported_in(&self, level: SecurityLevel, active: S) -> bool {
        level.request_seed() != 0x05 || matches!(active, S::ProgrammingSession)
    }
    fn policy(&self, level: SecurityLevel) -> SecurityPolicy {
        match level.request_seed() {
            0x01 => SecurityPolicy::Counted {
                attempt_limit: 3,
                delay_ms: Some(10_000),
                static_seed: false,
            },
            _ => SecurityPolicy::RandomSeedOnly,
        }
    }
    fn load_attempts(&self, _l: SecurityLevel) -> u8 {
        self.attempts
    }
    fn store_attempts(&mut self, _l: SecurityLevel, count: u8) {
        self.attempts = count;
    }
    fn delay_running(&self, level: SecurityLevel) -> bool {
        self.delay && level.request_seed() == 0x01
    }
    fn start_delay(&mut self, _l: SecurityLevel) {
        self.delay = true;
        self.delays_started = self.delays_started.saturating_add(1);
    }
    async fn seed(
        &mut self,
        level: SecurityLevel,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let _ = out.write_all(&seed_of(level).to_be_bytes());
        Ok(())
    }
    async fn verify_key(
        &mut self,
        level: SecurityLevel,
        key: &[u8],
    ) -> Result<KeyVerdict, Nrc> {
        Ok(if key == key_of(level) {
            KeyVerdict::Valid
        } else {
            KeyVerdict::Invalid
        })
    }
}

fn seed_of(level: SecurityLevel) -> u16 {
    match level.request_seed() {
        0x01 => 0x3657,
        _ => 0x1234,
    }
}

fn key_of(level: SecurityLevel) -> [u8; 2] {
    seed_of(level).wrapping_neg().to_be_bytes()
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
    Ecu: DiagnosticSessionControl, TesterPresent, EcuReset, SecurityAccess;
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

// --- SecurityAccess (0x27), ISO 14229-1:2020 clause 10.4 and Annex I -------------------

const RIGHT_KEY: [u8; 4] = [0x27, 0x02, 0xC9, 0xA9];
const WRONG_KEY: [u8; 4] = [0x27, 0x02, 0x00, 0x00];

fn extended(ecu: &mut Ecu) -> State {
    in_session(ecu, S::ExtendedDiagnosticSession)
}

/// Clause 10.4.5.2, Tables 47-50, then 10.4.5.3, Tables 51-52 — the seed, the key that
/// unlocks, and a zero seed for the level now unlocked (Annex I transitions 2, 3, 7).
#[test]
fn security_access_seed_key_then_a_zero_seed() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x36, 0x57][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x67, 0x02][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x00, 0x00][..])
    );
}

/// ``UDSSVC_ARCH_0007`` row 2, clause 10.4.4 — a level the server does not support,
/// asked by its `requestSeed` or its `sendKey`, and a reserved sub-function, are 0x12.
#[test]
fn security_access_an_unsupported_level_is_0x12() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x27, 0x07][..],
        &[0x27, 0x08, 0x00][..],
        &[0x27, 0x00][..],
        &[0x27, 0x7F][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x27, 0x12][..]),
            "{request:02X?}"
        );
    }
}

/// ``UDSSVC_ARCH_0007`` row 4 — a level supported, but not in the active session, is
/// 0x7E; it proceeds from the session that offers it.
#[test]
fn security_access_a_level_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x05]).as_deref(),
        Some(&[0x7F, 0x27, 0x7E][..])
    );
    let mut state = in_session(&mut ecu, S::ProgrammingSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x05]).as_deref(),
        Some(&[0x67, 0x05, 0x12, 0x34][..])
    );
}

/// Annex I transition 4 — a `sendKey` with no seed sent is 0x24, before its length is
/// checked (Figure 6 puts the sequence check ahead of the service-specific checks).
#[test]
fn security_access_a_key_without_a_seed_is_0x24() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [&RIGHT_KEY[..], &[0x27, 0x02][..]] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x27, 0x24][..]),
            "{request:02X?}"
        );
    }
}

/// Annex I transition 9 — a `sendKey` whose `yy` is not `xx + 1` is 0x24, and the seed
/// is discarded: the right key for the seeded level is then 0x24 too.
#[test]
fn security_access_a_key_for_another_level_is_0x24_and_discards_the_seed() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
}

/// Annex I transitions 4 and 9, clause 10.4.4 — this server takes no
/// `securityAccessDataRecord`, so a `requestSeed` carrying one is 0x13; so is an empty
/// key or one longer than `MAX_KEY_LEN`, and that `sendKey` still discards the seed.
#[test]
fn security_access_a_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01, 0xAA]).as_deref(),
        Some(&[0x7F, 0x27, 0x13][..])
    );
    for request in [&[0x27, 0x02][..], &[0x27, 0x02, 0xC9, 0xA9, 0x00][..]] {
        let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x27, 0x13][..]),
            "{request:02X?}"
        );
        assert_eq!(
            exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
            Some(&[0x7F, 0x27, 0x24][..])
        );
    }
}

/// Annex I transition 9 — a wrong key under the limit is 0x35, counts an attempt, and
/// discards the seed, so the client must ask for a new one (clause 10.4.1).
#[test]
fn security_access_a_wrong_key_is_0x35_and_discards_the_seed() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &WRONG_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x35][..])
    );
    assert_eq!(ecu.attempts, 1);
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
}

/// Annex I transition 9 and ``UDSSVC_ARCH_0037`` — the attempt for which
/// `(Att_Cnt + 1) >= Att_Cnt_Limit` is 0x36, starts the delay, and clamps the count at
/// the limit; a `requestSeed` while the delay runs is 0x37 (transition 4).
#[test]
fn security_access_the_attempt_at_the_limit_is_0x36_then_the_delay_is_0x37() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for code in [0x35, 0x35, 0x36] {
        let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
        assert_eq!(
            exchange(&mut ecu, &mut state, &WRONG_KEY).as_deref(),
            Some(&[0x7F, 0x27, code][..])
        );
    }
    assert_eq!((ecu.attempts, ecu.delays_started), (3, 1));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x37][..])
    );
}

/// Annex I, "Delay Timer Expiration Occurs" — once the delay has run out the count is
/// reset, so the next wrong key is 0x35 again rather than 0x36.
#[test]
fn security_access_an_expired_delay_resets_the_attempt_count() {
    let mut ecu = Ecu {
        attempts: 3,
        ..Ecu::default()
    };
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(ecu.attempts, 0);
    assert_eq!(
        exchange(&mut ecu, &mut state, &WRONG_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x35][..])
    );
}

/// Annex I transition 3 — a valid key resets the attempt count.
#[test]
fn security_access_a_valid_key_resets_the_attempt_count() {
    let mut ecu = Ecu {
        attempts: 2,
        ..Ecu::default()
    };
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x67, 0x02][..])
    );
    assert_eq!(ecu.attempts, 0);
}

/// A level whose policy counts nothing answers every wrong key 0x35, and never starts a
/// delay (Table I.1's fallback).
#[test]
fn security_access_an_uncounted_level_is_always_0x35() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for _ in 0..4 {
        let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
        assert_eq!(
            exchange(&mut ecu, &mut state, &[0x27, 0x04, 0x00, 0x00]).as_deref(),
            Some(&[0x7F, 0x27, 0x35][..])
        );
    }
    assert_eq!((ecu.attempts, ecu.delays_started), (0, 0));
}

/// Annex I transitions 8 and 10, clause 10.4.1 — with level 0x01 unlocked, unlocking
/// 0x03 locks 0x01: only one level is active, so 0x01's seed is real again.
#[test]
fn security_access_unlocking_another_level_locks_the_first() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x03]).as_deref(),
        Some(&[0x67, 0x03, 0x12, 0x34][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]).as_deref(),
        Some(&[0x67, 0x04][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x36, 0x57][..])
    );
}

/// Annex I transition 10 — a wrong key in state D discards the seed but keeps the level
/// already unlocked.
#[test]
fn security_access_a_wrong_key_while_unlocked_keeps_the_unlocked_level() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x04, 0x00, 0x00]).as_deref(),
        Some(&[0x7F, 0x27, 0x35][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x00, 0x00][..])
    );
}

/// Annex I transition 6, ``UDSSVC_ARCH_0038`` — an accepted session change locks every
/// level and discards a seed, and `on_transition` is told a level was relocked.
#[test]
fn security_access_a_session_change_locks() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(ecu.relocked, Some(false));
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    ecu.session_confirmed(&mut state, S::ExtendedDiagnosticSession);
    assert_eq!(ecu.relocked, Some(true));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x36, 0x57][..])
    );
}

/// Annex I transition 6 — a session timeout locks as a session change does.
#[test]
fn security_access_a_session_timeout_locks() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    ecu.session_timed_out(&mut state);
    assert_eq!(ecu.relocked, Some(true));
    ecu.session_confirmed(&mut state, S::ExtendedDiagnosticSession);
    assert_eq!(ecu.relocked, Some(false));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x67, 0x01, 0x36, 0x57][..])
    );
}
