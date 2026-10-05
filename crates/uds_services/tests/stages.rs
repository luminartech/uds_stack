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
    Address, Ai, CommunicationControl, CommunicationControlType, CommunicationType,
    ControlDtcSetting, DiagnosticSessionType as S, DtcSettingType, EcuReset, KeyVerdict,
    Mtype, ProtocolState, ResetType, Responded, ResponseSink, SecurityAccess,
    SecurityLevel, SecurityPolicy, ServiceSet, SessionTiming, SessionTransition, Sink,
    SubnetNumber, TaType, TesterPresent, uds_server,
};

#[derive(Debug, Default)]
struct Ecu {
    /// The reset `EcuReset::reset` last accepted.
    accepted: Option<ResetType>,
    /// Makes the next handler that can refuse answer `conditionsNotCorrect` (0x22).
    refuse: bool,
    /// Level 0x01's stored attempt count.
    attempts: u8,
    /// Whether level 0x01's delay is running.
    delay: bool,
    /// How many times a delay was started.
    delays_started: u8,
    /// The `security_relocked` of the last session transition.
    relocked: Option<bool>,
    /// Makes Annex I's optional pre-conditions unmet.
    preconditions_unmet: bool,
    /// The `securityAccessDataRecord` the last seed was asked with.
    record: Vec<u8>,
    /// What `CommunicationControl` last applied.
    communication: Option<(
        CommunicationControlType,
        CommunicationType,
        SubnetNumber,
        Option<u16>,
    )>,
    /// The `DTCSettingType` `ControlDtcSetting` last applied.
    dtc_setting: Option<DtcSettingType>,
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
    /// A soft or key-off-on reset requires level 0x03 unlocked.
    fn required_level(&self, kind: ResetType) -> Option<SecurityLevel> {
        match kind {
            ResetType::SoftReset | ResetType::KeyOffOnReset => {
                SecurityLevel::from_request_seed(0x03)
            }
            _ => None,
        }
    }
    async fn reset(
        &mut self,
        kind: ResetType,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        if core::mem::take(&mut self.refuse) {
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
/// seed's two's complement (clause 10.4.5.1). A `securityAccessDataRecord` of up to two
/// bytes identifies the client, and only `0E 80` is a client this server knows.
impl SecurityAccess for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_SEED_LEN: usize = 2;
    const MAX_KEY_LEN: usize = 2;
    const MAX_RECORD_LEN: usize = 2;
    fn supports(&self, level: SecurityLevel) -> bool {
        matches!(level.request_seed(), 0x01 | 0x03 | 0x05)
    }
    fn supported_in(&self, level: SecurityLevel, active: S) -> bool {
        level.request_seed() != 0x05 || matches!(active, S::ProgrammingSession)
    }
    fn preconditions_met(&self, _l: SecurityLevel) -> bool {
        !self.preconditions_unmet
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
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        if !matches!(record, [] | [0x0E, 0x80]) {
            return Err(Nrc::RequestOutOfRange);
        }
        self.record = record.to_vec();
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

/// Every `controlType` but `disableRxAndEnableTx` (0x02): `enableRxAndTx...` with
/// enhanced address information (0x05) is offered only in the programming session, and
/// `disableRxAndTx` (0x03) requires level 0x03. Node `0xFFFF` is one this server does not
/// know.
impl CommunicationControl for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn supports(&self, kind: CommunicationControlType) -> bool {
        matches!(u8::from(kind), 0x00 | 0x01 | 0x03 | 0x04 | 0x05)
    }
    fn supported_in(&self, kind: CommunicationControlType, active: S) -> bool {
        u8::from(kind) != 0x05 || matches!(active, S::ProgrammingSession)
    }
    fn required_level(&self, kind: CommunicationControlType) -> Option<SecurityLevel> {
        match kind {
            CommunicationControlType::DisableRxAndTx => {
                SecurityLevel::from_request_seed(0x03)
            }
            _ => None,
        }
    }
    async fn control(
        &mut self,
        control_type: CommunicationControlType,
        communication_type: CommunicationType,
        subnet: SubnetNumber,
        node_id: Option<u16>,
    ) -> Result<(), Nrc> {
        if core::mem::take(&mut self.refuse) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        if node_id == Some(0xFFFF) {
            return Err(Nrc::RequestOutOfRange);
        }
        self.communication = Some((control_type, communication_type, subnet, node_id));
        Ok(())
    }
}

/// On, off, and two vehicle-manufacturer settings: `0x40`, offered only in the
/// programming session, and `0x41`, which requires level 0x03. An option record of up to
/// three bytes names DTCs, and `FF` names none this server has.
impl ControlDtcSetting for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_OPTION_RECORD_LEN: usize = 3;
    fn supports(&self, setting: DtcSettingType) -> bool {
        matches!(u8::from(setting), 0x01 | 0x02 | 0x40 | 0x41)
    }
    fn supported_in(&self, setting: DtcSettingType, active: S) -> bool {
        u8::from(setting) != 0x40 || matches!(active, S::ProgrammingSession)
    }
    fn required_level(&self, setting: DtcSettingType) -> Option<SecurityLevel> {
        match u8::from(setting) {
            0x41 => SecurityLevel::from_request_seed(0x03),
            _ => None,
        }
    }
    async fn control_dtc_setting(
        &mut self,
        setting: DtcSettingType,
        option_record: &[u8],
    ) -> Result<(), Nrc> {
        if core::mem::take(&mut self.refuse) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        if option_record.contains(&0xFF) {
            return Err(Nrc::RequestOutOfRange);
        }
        self.dtc_setting = Some(setting);
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
    Ecu: DiagnosticSessionControl, TesterPresent, EcuReset, SecurityAccess,
         CommunicationControl, ControlDtcSetting;
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
/// 0x7E, ahead of Figure 6's security check though its level is locked too; the same
/// request proceeds from the session that offers it, once the level it requires is
/// unlocked.
#[test]
fn ecu_reset_a_reset_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x02]).as_deref(),
        Some(&[0x7F, 0x11, 0x7E][..])
    );
    let mut state = in_session(&mut ecu, S::ExtendedDiagnosticSession);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]);
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
        refuse: true,
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
    assert_eq!(exchange(&mut ecu, &mut state, &[0x11, 0x81]), None);
    assert_eq!(ecu.accepted, Some(ResetType::HardReset));
}

/// Figure 6's sub-function security check — a reset requiring a level that is locked is
/// 0x33, even with a trailing byte, and the handler is not asked; once that level is
/// unlocked the same reset proceeds.
#[test]
fn ecu_reset_a_reset_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = in_session(&mut ecu, S::ExtendedDiagnosticSession);
    for request in [
        &[0x11, 0x03][..],
        &[0x11, 0x83, 0x00][..],
        &[0x11, 0x02][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x11, 0x33][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.accepted, None);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x03]).as_deref(),
        Some(&[0x51, 0x03][..])
    );
}

/// Figure 6 — a reset requiring one level is 0x33 while another is unlocked.
#[test]
fn ecu_reset_another_unlocked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = in_session(&mut ecu, S::ExtendedDiagnosticSession);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x11, 0x03]).as_deref(),
        Some(&[0x7F, 0x11, 0x33][..])
    );
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

/// Annex I transitions 4 and 9, clause 10.4.4 — a `securityAccessDataRecord` longer than
/// `MAX_RECORD_LEN` is 0x13 and the application is not asked; so is an empty key or one
/// longer than `MAX_KEY_LEN`, and that `sendKey` still discards the seed.
#[test]
fn security_access_a_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01, 0x0E, 0x80, 0x00]).as_deref(),
        Some(&[0x7F, 0x27, 0x13][..])
    );
    assert_eq!(ecu.record, [0_u8; 0]);
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

/// Clause 10.4.2.3 and 10.4.4 — a `securityAccessDataRecord` reaches the application with
/// the `requestSeed`, and one holding data it rejects is its 0x31, which sends no seed:
/// from state A, the key that follows is 0x24.
#[test]
fn security_access_the_data_record_reaches_the_seed() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01, 0x0F]).as_deref(),
        Some(&[0x7F, 0x27, 0x31][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01, 0x0E, 0x80]).as_deref(),
        Some(&[0x67, 0x01, 0x36, 0x57][..])
    );
    assert_eq!(ecu.record, [0x0E, 0x80]);
}

/// Annex I Table I.2 transition 4 — unmet optional pre-conditions are 0x22, ahead of the
/// delay's 0x37; transition 7's zero seed is not reached either.
#[test]
fn security_access_unmet_preconditions_are_0x22_before_the_delay() {
    let mut ecu = Ecu {
        delay: true,
        preconditions_unmet: true,
        ..Ecu::default()
    };
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x22][..])
    );
    ecu.preconditions_unmet = false;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x37][..])
    );
    ecu.delay = false;
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    let _ = exchange(&mut ecu, &mut state, &RIGHT_KEY);
    ecu.preconditions_unmet = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x22][..])
    );
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

// --- CommunicationControl (0x28), ISO 14229-1:2020 clause 10.5 -------------------------

/// Clause 10.2 Table 23 — the service is not applicable in the default session: 0x7F.
#[test]
fn communication_control_is_0x7f_in_the_default_session() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x00, 0x01]).as_deref(),
        Some(&[0x7F, 0x28, 0x7F][..])
    );
}

/// ``UDSSVC_ARCH_0007`` row 2, clause 10.5.4 — a `controlType` the server does not
/// support, and a reserved one, are 0x12 before the length is checked, and the handler
/// is not asked.
#[test]
fn communication_control_an_unsupported_control_type_is_0x12() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x28, 0x02, 0x01][..],
        &[0x28, 0x06, 0x01][..],
        &[0x28, 0x7F, 0x01][..],
        &[0x28, 0x82][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x28, 0x12][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.communication, None);
}

/// ``UDSSVC_ARCH_0007`` row 4 — a `controlType` supported, but not in the active session,
/// is 0x7E; it proceeds from the session that offers it.
#[test]
fn communication_control_a_control_type_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let request = [0x28, 0x05, 0x01, 0x00, 0x0A];
    assert_eq!(
        exchange(&mut ecu, &mut state, &request).as_deref(),
        Some(&[0x7F, 0x28, 0x7E][..])
    );
    let mut state = in_session(&mut ecu, S::ProgrammingSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &request).as_deref(),
        Some(&[0x68, 0x05][..])
    );
}

/// Figure 6's sub-function security check — a `controlType` requiring a locked level is
/// 0x33, and proceeds once that level is unlocked.
#[test]
fn communication_control_a_control_type_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x03, 0x01]).as_deref(),
        Some(&[0x7F, 0x28, 0x33][..])
    );
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x03, 0x01]).as_deref(),
        Some(&[0x68, 0x03][..])
    );
}

/// Clause 10.5.2.1, Table 53 — `nodeIdentificationNumber` is present exactly for the
/// enhanced-address `controlType`s: a missing one, a missing `communicationType`, or a
/// trailing byte is 0x13.
#[test]
fn communication_control_a_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x28, 0x04, 0x01][..],
        &[0x28, 0x04, 0x01, 0x00][..],
        &[0x28, 0x00][..],
        &[0x28, 0x00, 0x01, 0x00][..],
        &[0x28, 0x04, 0x01, 0x00, 0x0A, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x28, 0x13][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.communication, None);
}

/// Clause 10.5.4 — the handler's 0x31 for an error in `nodeIdentificationNumber`, and its
/// 0x22 where it cannot switch the communication, are the response.
#[test]
fn communication_control_the_handler_refusals_are_its_codes() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x04, 0x01, 0xFF, 0xFF]).as_deref(),
        Some(&[0x7F, 0x28, 0x31][..])
    );
    ecu.refuse = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x01, 0x02]).as_deref(),
        Some(&[0x7F, 0x28, 0x22][..])
    );
    assert_eq!(ecu.communication, None);
}

/// Clause 10.5.5 and 10.5.6, Tables 59-62 — the positive response echoes the
/// `controlType`; the handler receives the `communicationType`, the subnet from its high
/// nibble, and the node only where the request carried one. The suppress bit silences
/// the response.
#[test]
fn communication_control_the_positive_response_echoes_the_control_type() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x01, 0xF2]).as_deref(),
        Some(&[0x68, 0x01][..])
    );
    assert_eq!(
        ecu.communication,
        Some((
            CommunicationControlType::EnableRxAndDisableTx,
            CommunicationType::NetworkManagement,
            SubnetNumber::ReceivedOn,
            None,
        ))
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x28, 0x04, 0x01, 0x00, 0x0A]).as_deref(),
        Some(&[0x68, 0x04][..])
    );
    assert_eq!(
        ecu.communication,
        Some((
            CommunicationControlType::EnableRxAndDisableTxWithEnhancedAddressInfo,
            CommunicationType::Normal,
            SubnetNumber::AllConnectedNetworks,
            Some(0x000A),
        ))
    );
    assert_eq!(exchange(&mut ecu, &mut state, &[0x28, 0x80, 0x03]), None);
    assert_eq!(
        ecu.communication.map(|(kind, ..)| kind),
        Some(CommunicationControlType::EnableRxAndTx)
    );
}

// --- ControlDTCSetting (0x85), ISO 14229-1:2020 clause 10.8 ----------------------------

/// Clause 10.2 Table 23 — the service is not applicable in the default session: 0x7F,
/// before its sub-function is looked at.
#[test]
fn control_dtc_setting_is_0x7f_in_the_default_session() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [&[0x85, 0x02][..], &[0x85, 0x00][..]] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x85, 0x7F][..]),
            "{request:02X?}"
        );
    }
}

/// ``UDSSVC_ARCH_0007`` row 2, clause 10.8.4 — a reserved `DTCSettingType`, and a
/// manufacturer one the server does not support, are 0x12, with or without an option
/// record, and the handler is not asked.
#[test]
fn control_dtc_setting_an_unsupported_setting_is_0x12() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x85, 0x00][..],
        &[0x85, 0x03][..],
        &[0x85, 0x7F][..],
        &[0x85, 0x42][..],
        &[0x85, 0x83, 0x01, 0x02, 0x03, 0x04][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x85, 0x12][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.dtc_setting, None);
}

/// ``UDSSVC_ARCH_0007`` row 4 — a setting supported, but not in the active session, is
/// 0x7E; it proceeds from the session that offers it.
#[test]
fn control_dtc_setting_a_setting_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x40]).as_deref(),
        Some(&[0x7F, 0x85, 0x7E][..])
    );
    let mut state = in_session(&mut ecu, S::ProgrammingSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x40]).as_deref(),
        Some(&[0xC5, 0x40][..])
    );
}

/// Figure 6's sub-function security check — a setting requiring a locked level is 0x33,
/// and proceeds once that level is unlocked.
#[test]
fn control_dtc_setting_a_setting_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x41]).as_deref(),
        Some(&[0x7F, 0x85, 0x33][..])
    );
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x03]);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x41]).as_deref(),
        Some(&[0xC5, 0x41][..])
    );
}

/// Clause 10.8.4 — an option record longer than `MAX_OPTION_RECORD_LEN` is 0x13, and
/// the handler is not asked.
#[test]
fn control_dtc_setting_an_overlong_option_record_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x02, 0x01, 0x02, 0x03, 0x04]).as_deref(),
        Some(&[0x7F, 0x85, 0x13][..])
    );
    assert_eq!(ecu.dtc_setting, None);
}

/// Clause 10.8.4 — the handler's 0x31 for an error in the option record, and its 0x22
/// where it cannot perform the control, are the response.
#[test]
fn control_dtc_setting_the_handler_refusals_are_its_codes() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x02, 0xFF]).as_deref(),
        Some(&[0x7F, 0x85, 0x31][..])
    );
    ecu.refuse = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x02]).as_deref(),
        Some(&[0x7F, 0x85, 0x22][..])
    );
    assert_eq!(ecu.dtc_setting, None);
}

/// Clause 10.8.3, Table 130, and 10.8.5 — the positive response echoes the
/// `DTCSettingType`, with or without an option record; the suppress bit silences it.
#[test]
fn control_dtc_setting_the_positive_response_echoes_the_setting() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x02]).as_deref(),
        Some(&[0xC5, 0x02][..])
    );
    assert_eq!(ecu.dtc_setting, Some(DtcSettingType::Off));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x85, 0x01, 0x12, 0x34, 0x56]).as_deref(),
        Some(&[0xC5, 0x01][..])
    );
    assert_eq!(exchange(&mut ecu, &mut state, &[0x85, 0x82]), None);
    assert_eq!(ecu.dtc_setting, Some(DtcSettingType::Off));
}
