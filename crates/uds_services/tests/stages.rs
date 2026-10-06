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
    Access, Address, AfterSend, Ai, ClearDiagnosticInformation, CommunicationControl,
    CommunicationControlType, CommunicationType, ControlDtcSetting, DataIdentifier, Delay,
    DiagnosticSessionType as S, DtcRecord, DtcReportKind, DtcSettingType, DtcStatusMask,
    EcuReset, KeyVerdict, Levels, Mtype, ProtocolState, ReadDtcInfoReportType,
    ReadDtcInfoSubFunction, ReadDtcInformation, Received, RecordError, ResetType,
    Responded, ResponseSink, RoutineControl, RoutineControlSubFunction, RoutineIdentifier,
    SecurityAccess, SecurityLevel, SecurityPolicy, ServiceSet, SessionTiming,
    SessionTransition, Sessions, Sink, SubnetNumber, TaType, TesterPresent,
    WriteDataByIdentifier, uds_server,
};

/// The test server's data identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    /// `F190`, 17 bytes, writable only in the extended session.
    Vin,
    /// `F40D`, one byte, read-only.
    VehicleSpeed,
    /// `0100`, two bytes, written only with level 0x03 unlocked.
    Config,
    /// `0101`, one byte, and only `00`-`03` is a valid mode.
    Mode,
}

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 17;
    fn as_u16(self) -> u16 {
        match self {
            Self::Vin => 0xF190,
            Self::VehicleSpeed => 0xF40D,
            Self::Config => 0x0100,
            Self::Mode => 0x0101,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xF190 => Some(Self::Vin),
            0xF40D => Some(Self::VehicleSpeed),
            0x0100 => Some(Self::Config),
            0x0101 => Some(Self::Mode),
            _ => None,
        }
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        let width = match self {
            Self::Vin => 17,
            Self::Config => 2,
            Self::VehicleSpeed | Self::Mode => 1,
        };
        let (record, rest) = buf.split_at_checked(width).ok_or(RecordError::Short)?;
        match (self, record) {
            (Self::Mode, [mode]) if *mode > 0x03 => {
                Err(RecordError::Malformed { len: width })
            }
            _ => Ok((record, rest)),
        }
    }
}

/// The test server's routines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rid {
    /// `FF00`, started only with level 0x03 unlocked.
    EraseMemory,
    /// `0201`, started with a one-byte option record and then asked for results; it
    /// cannot be stopped.
    SelfTest,
    /// `0202`, available only in the programming session.
    CheckProgramming,
}

impl RoutineIdentifier for Rid {
    const MAX_STATUS_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        match self {
            Self::EraseMemory => 0xFF00,
            Self::SelfTest => 0x0201,
            Self::CheckProgramming => 0x0202,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xFF00 => Some(Self::EraseMemory),
            0x0201 => Some(Self::SelfTest),
            0x0202 => Some(Self::CheckProgramming),
            _ => None,
        }
    }
}

/// A way `SecurityAccess::seed` can misbehave.
#[derive(Debug, Default, Clone, Copy)]
enum SeedFault {
    #[default]
    None,
    /// An all-zero seed, which clause 10.4.1 forbids for a locked level.
    Zero,
    /// `00 AB` written a byte at a time, so a short sink refuses it part-way.
    Split,
}

#[derive(Debug, Default)]
struct Ecu {
    /// The reset `EcuReset::reset` last accepted.
    accepted: Option<ResetType>,
    /// Makes the next handler that can refuse answer `conditionsNotCorrect` (0x22).
    refuse: bool,
    /// Level 0x01's stored attempt count.
    attempts: u8,
    /// Level 0x01's delay timer.
    delay: Delay,
    /// How many times a delay was started.
    delays_started: u8,
    /// The `security_relocked` of the last session transition.
    relocked: Option<bool>,
    /// The session the last transition entered.
    entered: Option<S>,
    /// Makes Annex I's optional pre-conditions unmet.
    preconditions_unmet: bool,
    /// How `seed` misbehaves, if it does.
    seed_fault: SeedFault,
    /// The `securityAccessDataRecord` the last seed was asked with.
    record: Vec<u8>,
    /// What `CommunicationControl` last applied.
    communication: Option<(
        CommunicationControlType,
        CommunicationType,
        SubnetNumber,
        Option<u16>,
    )>,
    /// What `WriteDataByIdentifier` last stored.
    written: Option<(Did, Vec<u8>)>,
    /// The routine `RoutineControl` last started.
    started: Option<Rid>,
    /// What `ClearDiagnosticInformation` last cleared.
    cleared: Option<(DtcRecord, Option<u8>)>,
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
    fn leaves_running_software(&self, _s: S) -> bool {
        false
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, entered: S, relocked: bool) {
        self.relocked = Some(relocked);
        self.entered = Some(entered);
    }
}

fn everywhere() -> Access {
    Access::new(Sessions::ALL)
}

fn only_in(session: S) -> Access {
    Access::new(Sessions::of(&[session]))
}

#[allow(clippy::expect_used, reason = "0x03 is a requestSeed value")]
fn by_level_3(access: Access) -> Access {
    access.unlocked_by(
        Levels::NONE
            .with(SecurityLevel::from_request_seed(0x03).expect("a requestSeed value")),
    )
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    /// A key-off-on reset is offered only in the extended session, and it and a soft
    /// reset require level 0x03 unlocked.
    fn access(&self, kind: ResetType) -> Option<Access> {
        match kind {
            ResetType::HardReset | ResetType::EnableRapidPowerShutDown => {
                Some(everywhere())
            }
            ResetType::SoftReset => Some(by_level_3(everywhere())),
            ResetType::KeyOffOnReset => {
                Some(by_level_3(only_in(S::ExtendedDiagnosticSession)))
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
    fn sessions(&self, level: SecurityLevel) -> Option<Sessions> {
        match level.request_seed() {
            0x01 | 0x03 => Some(Sessions::ALL),
            0x05 => Some(Sessions::of(&[S::ProgrammingSession])),
            _ => None,
        }
    }
    fn preconditions_met(&self, _l: SecurityLevel) -> bool {
        !self.preconditions_unmet
    }
    fn policy(&self, level: SecurityLevel) -> SecurityPolicy {
        match level.request_seed() {
            0x01 => SecurityPolicy::Counted {
                attempt_limit: core::num::NonZeroU8::MIN.saturating_add(2),
                delay_ms: Some(10_000),
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
    fn delay(&mut self, level: SecurityLevel) -> Delay {
        if level.request_seed() != 0x01 {
            return Delay::Idle;
        }
        let delay = self.delay;
        if delay == Delay::Expired {
            self.delay = Delay::Idle;
        }
        delay
    }
    fn start_delay(&mut self, _l: SecurityLevel) {
        self.delay = Delay::Running;
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
        match self.seed_fault {
            SeedFault::None => {
                let _ = out.write_all(&seed_of(level).to_be_bytes());
            }
            SeedFault::Zero => {
                let _ = out.write_all(&[0x00, 0x00]);
            }
            SeedFault::Split => {
                let _ = out.write_all(&[0x00]);
                let _ = out.write_all(&[0xAB]);
            }
        }
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

/// The three DTCs of clause 12.3.5.2's example, and its `DTCStatusAvailabilityMask`.
const DTCS: [([u8; 3], u8); 3] = [
    ([0x08, 0x05, 0x11], 0x24),
    ([0x0A, 0x9B, 0x17], 0x26),
    ([0x25, 0x22, 0x1F], 0x2F),
];
const AVAILABILITY: u8 = 0x2F;

/// Reports `01`, `02`, `0A` and `19`: `reportSupportedDTC` (0x0A) only in the extended
/// session, and the user-defined memory report (0x19) only with level 0x03 unlocked.
impl ReadDtcInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DTCS: usize = 3;
    const REPORTS: &'static [DtcReportKind] =
        &[DtcReportKind::Count, DtcReportKind::DtcList];
    fn access(&self, report: ReadDtcInfoReportType) -> Option<Access> {
        match u8::from(report) {
            0x01 | 0x02 => Some(everywhere()),
            0x0A => Some(only_in(S::ExtendedDiagnosticSession)),
            0x19 => Some(by_level_3(everywhere())),
            _ => None,
        }
    }
    async fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let matching = |mask: DtcStatusMask| {
            DTCS.into_iter()
                .filter(move |&(_, status)| status & u8::from(mask) != 0)
        };
        match request {
            ReadDtcInfoSubFunction::ReportNumberOfDtcByStatusMask(mask) => {
                let count = u16::try_from(matching(mask).count()).unwrap_or(u16::MAX);
                let _ = out.write_all(&[AVAILABILITY, 0x01]);
                let _ = out.write_all(&count.to_be_bytes());
            }
            ReadDtcInfoSubFunction::ReportDtcByStatusMask(mask) => {
                let _ = out.write_all(&[AVAILABILITY]);
                for (dtc, status) in matching(mask) {
                    let _ = out.write_all(&dtc);
                    let _ = out.write_all(&[status]);
                }
            }
            ReadDtcInfoSubFunction::ReportSupportedDtc => {
                let _ = out.write_all(&[AVAILABILITY]);
            }
            ReadDtcInfoSubFunction::ReportUserDefMemoryDtcExtDataRecordByDtcNumber(
                dtc,
                _,
                memory,
            ) => {
                let bytes = [dtc.high_byte(), dtc.middle_byte(), dtc.low_byte()];
                let Some((_, status)) = DTCS.into_iter().find(|&(d, _)| d == bytes) else {
                    return Err(Nrc::RequestOutOfRange);
                };
                let _ = out.write_all(&[memory]);
                let _ = out.write_all(&bytes);
                let _ = out.write_all(&[status]);
            }
            _ => return Err(Nrc::SubFunctionNotSupported),
        }
        Ok(())
    }
}

/// `routineInfo` is `00` throughout; the self-test's result is `5A`. A self-test option
/// record of `FF` is one it rejects.
impl RoutineControl for Ecu {
    type Rid = Rid;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_OPTION_LEN: usize = 4;
    fn access(&self, routine: Rid) -> Option<Access> {
        match routine {
            Rid::SelfTest => Some(everywhere()),
            Rid::EraseMemory => Some(by_level_3(everywhere())),
            Rid::CheckProgramming => Some(only_in(S::ProgrammingSession)),
        }
    }
    /// The self-test cannot be stopped; the others can only be started.
    fn supports(&self, routine: Rid, control: RoutineControlSubFunction) -> bool {
        match routine {
            Rid::SelfTest => !matches!(control, RoutineControlSubFunction::StopRoutine),
            Rid::EraseMemory | Rid::CheckProgramming => {
                matches!(control, RoutineControlSubFunction::StartRoutine)
            }
        }
    }
    async fn start(
        &mut self,
        routine: Rid,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        if matches!(routine, Rid::SelfTest) {
            match record {
                [0xFF] => return Err(Nrc::RequestOutOfRange),
                [_] => {}
                _ => return Err(Nrc::IncorrectMessageLengthOrInvalidFormat),
            }
        }
        if core::mem::take(&mut self.refuse) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        self.started = Some(routine);
        let _ = out.write_all(&[0x00]);
        Ok(())
    }
    /// Never asked: `supports` refuses every stop.
    async fn stop(
        &mut self,
        _routine: Rid,
        _record: &[u8],
        _out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        Err(Nrc::SubFunctionNotSupported)
    }
    async fn results(
        &mut self,
        routine: Rid,
        _record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        if self.started != Some(routine) {
            return Err(Nrc::RequestSequenceError);
        }
        let _ = out.write_all(&[0x00, 0x5A]);
        Ok(())
    }
}

/// Clears every group, the emissions-related group `FFFF33` (Annex D.1), and DTC
/// `012345`; memory `00` is the only user-defined DTC memory.
impl ClearDiagnosticInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = true;
    async fn clear(&mut self, group: DtcRecord, memory: Option<u8>) -> Result<(), Nrc> {
        if memory.is_some_and(|m| m != 0x00) {
            return Err(Nrc::RequestOutOfRange);
        }
        if ![
            uds_services::CLEAR_ALL_DTCS,
            DtcRecord::new(0xFF, 0xFF, 0x33),
            DtcRecord::new(0x01, 0x23, 0x45),
        ]
        .contains(&group)
        {
            return Err(Nrc::RequestOutOfRange);
        }
        if core::mem::take(&mut self.refuse) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        self.cleared = Some((group, memory));
        Ok(())
    }
}

impl WriteDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, did: Did) -> Option<Access> {
        match did {
            Did::Vin => Some(only_in(S::ExtendedDiagnosticSession)),
            Did::VehicleSpeed => None,
            Did::Config => Some(by_level_3(everywhere())),
            Did::Mode => Some(everywhere()),
        }
    }
    /// A `Config` of `FFFF` is a value this server rejects.
    async fn write(&mut self, did: Did, record: &[u8]) -> Result<(), Nrc> {
        if core::mem::take(&mut self.refuse) {
            return Err(Nrc::ConditionsNotCorrect);
        }
        if matches!(did, Did::Config) && record == [0xFF, 0xFF] {
            return Err(Nrc::RequestOutOfRange);
        }
        self.written = Some((did, record.to_vec()));
        Ok(())
    }
}

/// Every `controlType` but `disableRxAndEnableTx` (0x02): `enableRxAndTx...` with
/// enhanced address information (0x05) is offered only in the programming session, and
/// `disableRxAndTx` (0x03) requires level 0x03. Node `0xFFFF` is one this server does not
/// know.
impl CommunicationControl for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, kind: CommunicationControlType) -> Option<Access> {
        match u8::from(kind) {
            0x00 | 0x01 | 0x04 => Some(everywhere()),
            0x03 => Some(by_level_3(everywhere())),
            0x05 => Some(only_in(S::ProgrammingSession)),
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
    fn access(&self, setting: DtcSettingType) -> Option<Access> {
        match u8::from(setting) {
            0x01 | 0x02 => Some(everywhere()),
            0x40 => Some(only_in(S::ProgrammingSession)),
            0x41 => Some(by_level_3(everywhere())),
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
    async fn t_data_req(
        &mut self,
        _ai: Ai,
        _d: &[u8],
        _after: AfterSend,
    ) -> Result<(), ()> {
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
         CommunicationControl, ControlDtcSetting, WriteDataByIdentifier,
         ClearDiagnosticInformation, ReadDtcInformation, RoutineControl;
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
    let unsettled =
        block_on(ecu.dispatch(state, PHYSICAL, Received::Whole(request), &mut out));
    match settle(PHYSICAL, unsettled, false, &mut out) {
        Responded::Yes { .. } => Some(out.written_bytes().to_vec()),
        Responded::Suppressed { .. } => None,
    }
}

/// What the front of a request too long for the buffer produced, as the driver hands it
/// over for a `DataTooLong`.
fn exchange_truncated(ecu: &mut Ecu, state: &mut State, front: &[u8]) -> Vec<u8> {
    let mut buf = [0_u8; 64];
    let mut out = ResponseSink::new(&mut buf, None);
    let unsettled =
        block_on(ecu.dispatch(state, PHYSICAL, Received::Truncated(front), &mut out));
    let _ = settle(PHYSICAL, unsettled, false, &mut out);
    out.written_bytes().to_vec()
}

/// What a request produced through a sink bounded at `bound` bytes, as the peer's
/// advertised maximum bounds it.
fn exchange_bounded(
    ecu: &mut Ecu,
    state: &mut State,
    request: &[u8],
    bound: usize,
) -> Vec<u8> {
    let mut buf = [0_u8; 64];
    let mut out = ResponseSink::new(&mut buf, Some(bound));
    let unsettled =
        block_on(ecu.dispatch(state, PHYSICAL, Received::Whole(request), &mut out));
    let _ = settle(PHYSICAL, unsettled, false, &mut out);
    out.written_bytes().to_vec()
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

/// Annex I Table I.2 transition 9 — in state B, a `SecurityAccess` request refused for
/// any reason discards the seed, so the right key that follows is 0x24: a record too
/// long (0x13), the same too long to be received whole (0x13), unmet pre-conditions
/// (0x22), an unsupported level (0x12) and a level not offered in this session (0x7E). A
/// refused request for another service leaves the seed, and the key still unlocks.
#[test]
fn security_access_a_refused_request_discards_the_seed() {
    let mut ecu = Ecu::default();
    for refusal in [
        &[0x27, 0x01, 0x0E, 0x80, 0x00][..],
        &[0x27, 0x07][..],
        &[0x27, 0x05][..],
    ] {
        let mut state = extended(&mut ecu);
        let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
        let refused = exchange(&mut ecu, &mut state, refusal);
        assert_eq!(
            refused.as_deref().and_then(|bytes| bytes.get(..2)),
            Some(&[0x7F, 0x27][..]),
            "{refusal:02X?}"
        );
        assert_eq!(
            exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
            Some(&[0x7F, 0x27, 0x24][..]),
            "after {refusal:02X?}"
        );
    }

    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(
        exchange_truncated(&mut ecu, &mut state, &[0x27, 0x02, 0xC9, 0xA9]),
        [0x7F, 0x27, 0x13]
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );

    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    ecu.preconditions_unmet = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x22][..])
    );
    ecu.preconditions_unmet = false;
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );

    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x12, 0x34, 0x00]).as_deref(),
        Some(&[0x7F, 0x2E, 0x31][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x67, 0x02][..])
    );
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
        delay: Delay::Running,
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
    ecu.delay = Delay::Expired;
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

/// Annex I Table I.2 transition 1 — a restart loses a RAM delay timer but not the
/// stored attempt count, so a lockout at the limit is not mistaken for an expired delay:
/// start-up starts the delay again, and the `requestSeed` after it is 0x37. Without
/// start-up, the first `requestSeed` that finds the count at the limit with no delay
/// running starts it and is 0x37 too.
#[test]
fn security_access_a_lockout_outlives_a_power_cycle() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for _ in 0..3 {
        let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
        let _ = exchange(&mut ecu, &mut state, &WRONG_KEY);
    }
    assert_eq!((ecu.attempts, ecu.delay), (3, Delay::Running));

    ecu.delay = Delay::Idle;
    let mut state = State::INITIAL;
    ecu.start_up(&mut state);
    assert_eq!((ecu.attempts, ecu.delay), (3, Delay::Running));
    ecu.session_confirmed(&mut state, S::ExtendedDiagnosticSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x37][..])
    );

    ecu.delay = Delay::Idle;
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x27, 0x01]).as_deref(),
        Some(&[0x7F, 0x27, 0x37][..])
    );
    assert_eq!((ecu.attempts, ecu.delay), (3, Delay::Running));
}

/// Annex I, "Delay Timer Expiration Occurs" — once the delay has run out the count is
/// reset, so the next wrong key is 0x35 again rather than 0x36.
#[test]
fn security_access_an_expired_delay_resets_the_attempt_count() {
    let mut ecu = Ecu {
        attempts: 3,
        delay: Delay::Expired,
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

/// ``UDSSVC_ARCH_0005``, clause 10.5.4 and Annex B Table B.1 — a `communicationType`
/// with its reserved bits 3-2 set, or with the reserved value in bits 1-0, is an error in
/// that parameter: 0x31, not 0x13, and the handler is not asked.
#[test]
fn communication_control_a_reserved_communication_type_is_0x31() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x28, 0x00, 0x05][..],
        &[0x28, 0x04, 0x0D, 0x00, 0x0A][..],
        &[0x28, 0x01, 0x00][..],
        &[0x28, 0x01, 0xF0][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x28, 0x31][..]),
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

// --- WriteDataByIdentifier (0x2E), ISO 14229-1:2020 clause 11.7 ------------------------

/// Unlock level 0x03, from the extended session.
fn unlocked_level_3(ecu: &mut Ecu) -> State {
    let mut state = extended(ecu);
    let _ = exchange(ecu, &mut state, &[0x27, 0x03]);
    let _ = exchange(ecu, &mut state, &[0x27, 0x04, 0xED, 0xCC]);
    state
}

/// Figure 26, key 1 — a request without a data record is 0x13, before the identifier is
/// looked at.
#[test]
fn wdbi_a_request_without_a_record_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x2E, 0xF1, 0x90][..],
        &[0x2E, 0x12, 0x34][..],
        &[0x2E, 0xF1][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x2E, 0x13][..]),
            "{request:02X?}"
        );
    }
}

/// Figure 26, "DID supports service 2E in active session?", clause 11.7.4 — an identifier
/// the server does not define, a read-only one, and one not writable in the active
/// session are 0x31, before the record's length is checked.
#[test]
fn wdbi_an_identifier_not_writable_here_is_0x31() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x2E, 0x12, 0x34, 0x00][..],
        &[0x2E, 0xF4, 0x0D, 0x40][..],
        &[0x2E, 0xF4, 0x0D, 0x40, 0x41][..],
        &[0x2E, 0xF1, 0x90, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x2E, 0x31][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.written, None);
}

/// Figure 26, "DID supports service 2E in active session?" ahead of the total length —
/// a request too long for the buffer is 0x31 for an identifier not writable here, and
/// 0x13 only for one that is.
#[test]
fn wdbi_a_truncated_request_is_checked_for_its_identifier_before_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for (front, nrc) in [
        (&[0x2E, 0x12, 0x34, 0x00][..], 0x31),
        (&[0x2E, 0xF4, 0x0D, 0x40][..], 0x31),
        (&[0x2E, 0x01, 0x00, 0x12][..], 0x13),
    ] {
        assert_eq!(
            exchange_truncated(&mut ecu, &mut state, front),
            [0x7F, 0x2E, nrc],
            "{front:02X?}"
        );
    }
    assert_eq!(ecu.written, None);
}

/// Figure 26, key 2 — a record shorter than the identifier's, or followed by more bytes,
/// is 0x13, ahead of the security check.
#[test]
fn wdbi_a_record_of_the_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x2E, 0xF1, 0x90, 0x00, 0x01][..],
        &[0x2E, 0x01, 0x01, 0x00, 0x00][..],
        &[0x2E, 0x01, 0x00, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x2E, 0x13][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.written, None);
}

/// Figure 26, key 2 — the total length is checked before the record's content: a
/// malformed record followed by a stray byte is 0x13, and the same record alone is 0x31.
#[test]
fn wdbi_a_malformed_record_with_a_trailing_byte_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x01, 0x05, 0x00]).as_deref(),
        Some(&[0x7F, 0x2E, 0x13][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x01, 0x05]).as_deref(),
        Some(&[0x7F, 0x2E, 0x31][..])
    );
    assert_eq!(ecu.written, None);
}

/// Figure 26, "DID security check OK?" — an identifier whose level is locked is 0x33,
/// before its value is checked; it is written once that level is unlocked.
#[test]
fn wdbi_an_identifier_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x2E, 0x01, 0x00, 0x12, 0x34][..],
        &[0x2E, 0x01, 0x00, 0xFF, 0xFF][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x2E, 0x33][..]),
            "{request:02X?}"
        );
    }
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x00, 0x12, 0x34]).as_deref(),
        Some(&[0x6E, 0x01, 0x00][..])
    );
    assert_eq!(ecu.written, Some((Did::Config, vec![0x12, 0x34])));
}

/// Figure 26, "Data record is valid?", clause 11.7.4 — a record `split_record` finds
/// malformed is 0x31, and a value the handler rejects is its 0x31; the handler's 0x22 is
/// the response where it cannot write.
#[test]
fn wdbi_an_invalid_record_is_0x31_and_the_handler_refusal_is_its_code() {
    let mut ecu = Ecu::default();
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x01, 0x04]).as_deref(),
        Some(&[0x7F, 0x2E, 0x31][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x00, 0xFF, 0xFF]).as_deref(),
        Some(&[0x7F, 0x2E, 0x31][..])
    );
    ecu.refuse = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x01, 0x02]).as_deref(),
        Some(&[0x7F, 0x2E, 0x22][..])
    );
    assert_eq!(ecu.written, None);
}

/// Clause 11.7.5, Tables 282-283 — the VIN is written in the session that allows it, and
/// the positive response echoes the identifier.
#[test]
fn wdbi_the_positive_response_echoes_the_identifier() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    let mut request = vec![0x2E, 0xF1, 0x90];
    request.extend_from_slice(b"W0L000043MB541326");
    assert_eq!(
        exchange(&mut ecu, &mut state, &request).as_deref(),
        Some(&[0x6E, 0xF1, 0x90][..])
    );
    assert_eq!(ecu.written, Some((Did::Vin, b"W0L000043MB541326".to_vec())));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x2E, 0x01, 0x01, 0x03]).as_deref(),
        Some(&[0x6E, 0x01, 0x01][..])
    );
}

// --- ClearDiagnosticInformation (0x14), ISO 14229-1:2020 clause 12.2 -------------------

/// Figure 28, key 1 — a request that is neither four bytes nor five is 0x13, and the
/// handler is not asked.
#[test]
fn clear_dtc_a_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x14][..],
        &[0x14, 0xFF, 0xFF][..],
        &[0x14, 0xFF, 0xFF, 0xFF, 0x00, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x14, 0x13][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.cleared, None);
}

/// Figure 28 and clause 12.2.4 — an unsupported `MemorySelection` or `groupOfDTC` is the
/// handler's 0x31, and its 0x22 where it cannot clear.
#[test]
fn clear_dtc_the_handler_refusals_are_its_codes() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x14, 0xFF, 0xFF, 0xFF, 0x01][..],
        &[0x14, 0xFF, 0xFF, 0xD0][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x14, 0x31][..]),
            "{request:02X?}"
        );
    }
    ecu.refuse = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x14, 0xFF, 0xFF, 0xFF]).as_deref(),
        Some(&[0x7F, 0x14, 0x22][..])
    );
    assert_eq!(ecu.cleared, None);
}

/// Clause 12.2.5, Tables 300-301 — the emissions-related group is cleared in the default
/// session and answered `54`; a single DTC, and a user-defined memory, reach the handler.
#[test]
fn clear_dtc_the_positive_response_is_0x54() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x14, 0xFF, 0xFF, 0x33]).as_deref(),
        Some(&[0x54][..])
    );
    assert_eq!(ecu.cleared, Some((DtcRecord::new(0xFF, 0xFF, 0x33), None)));
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x14, 0x01, 0x23, 0x45, 0x00]).as_deref(),
        Some(&[0x54][..])
    );
    assert_eq!(
        ecu.cleared,
        Some((DtcRecord::new(0x01, 0x23, 0x45), Some(0x00)))
    );
}

// --- ReadDTCInformation (0x19), ISO 14229-1:2020 clause 12.3 ---------------------------

/// ``UDSSVC_ARCH_0007`` row 2, clause 12.3.4 — a report type the server does not support,
/// and a reserved one, are 0x12 whatever parameters follow, and the handler is not asked.
#[test]
fn read_dtc_an_unsupported_report_type_is_0x12() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x19, 0x03][..],
        &[0x19, 0x04][..],
        &[0x19, 0x00][..],
        &[0x19, 0x1B, 0x00][..],
        &[0x19, 0x7F][..],
        &[0x19, 0x83, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x19, 0x12][..]),
            "{request:02X?}"
        );
    }
}

/// ``UDSSVC_ARCH_0007`` row 4 — a report type supported, but not in the active session,
/// is 0x7E; it proceeds from the session that offers it.
#[test]
fn read_dtc_a_report_type_not_offered_in_the_active_session_is_0x7e() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x19, 0x0A]).as_deref(),
        Some(&[0x7F, 0x19, 0x7E][..])
    );
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x19, 0x0A]).as_deref(),
        Some(&[0x59, 0x0A, AVAILABILITY][..])
    );
}

/// Figure 6's sub-function security check — a report type requiring a locked level is
/// 0x33, even with its parameters missing, and proceeds once that level is unlocked.
#[test]
fn read_dtc_a_report_type_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x19, 0x19][..],
        &[0x19, 0x19, 0x25, 0x22, 0x1F, 0xFF, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x19, 0x33][..]),
            "{request:02X?}"
        );
    }
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange(
            &mut ecu,
            &mut state,
            &[0x19, 0x19, 0x25, 0x22, 0x1F, 0xFF, 0x00]
        )
        .as_deref(),
        Some(&[0x59, 0x19, 0x00, 0x25, 0x22, 0x1F, 0x2F][..])
    );
}

/// Clause 12.3.4 — a supported report type with a parameter missing, or followed by a
/// trailing byte, is 0x13.
#[test]
fn read_dtc_a_wrong_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [&[0x19, 0x01][..], &[0x19, 0x02, 0x08, 0x00][..]] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x19, 0x13][..]),
            "{request:02X?}"
        );
    }
}

/// Clause 12.3.4 — a `DTCMaskRecord` the server does not recognise is the handler's 0x31.
#[test]
fn read_dtc_an_unknown_dtc_is_the_handler_0x31() {
    let mut ecu = Ecu::default();
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange(
            &mut ecu,
            &mut state,
            &[0x19, 0x19, 0x12, 0x34, 0x56, 0xFF, 0x00]
        )
        .as_deref(),
        Some(&[0x7F, 0x19, 0x31][..])
    );
}

/// Clause 12.3.5.2, Tables 340-341 — the pipeline writes `59 01` and the handler the
/// availability mask, format and count; a report by status mask lists the matching
/// DTCs. The suppress bit silences the response.
#[test]
fn read_dtc_the_positive_response_follows_the_example() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x19, 0x01, 0x08]).as_deref(),
        Some(&[0x59, 0x01, 0x2F, 0x01, 0x00, 0x01][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x19, 0x02, 0x02]).as_deref(),
        Some(
            &[
                0x59, 0x02, 0x2F, 0x0A, 0x9B, 0x17, 0x26, 0x25, 0x22, 0x1F, 0x2F
            ][..]
        )
    );
    assert_eq!(exchange(&mut ecu, &mut state, &[0x19, 0x81, 0x08]), None);
}

// --- RoutineControl (0x31), ISO 14229-1:2020 clause 14.2 -------------------------------

/// Figure 30, key 1 — a request shorter than its routine identifier is 0x13.
#[test]
fn routine_control_a_request_without_its_identifier_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [&[0x31, 0x01][..], &[0x31, 0x01, 0x02][..]] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, 0x13][..]),
            "{request:02X?}"
        );
    }
}

/// Figure 30, "RID supported in active session?", clause 14.2.4 — an identifier the
/// server does not define, and one not available in the active session, are 0x31, before
/// the security and length checks.
#[test]
fn routine_control_a_routine_not_available_here_is_0x31() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x31, 0x01, 0x12, 0x34][..],
        &[0x31, 0x01, 0x02, 0x02][..],
        &[0x31, 0x01, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, 0x31][..]),
            "{request:02X?}"
        );
    }
    let mut state = in_session(&mut ecu, S::ProgrammingSession);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x01, 0x02, 0x02]).as_deref(),
        Some(&[0x71, 0x01, 0x02, 0x02, 0x00][..])
    );
}

/// Figure 30, "RID security check OK?" — a routine whose level is locked is 0x33, ahead
/// of the length check and of any sub-function's verdict; it runs once that level is
/// unlocked.
#[test]
fn routine_control_a_routine_requiring_a_locked_level_is_0x33() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for request in [
        &[0x31, 0x01, 0xFF, 0x00][..],
        &[0x31, 0x02, 0xFF, 0x00][..],
        &[0x31, 0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, 0x33][..]),
            "{request:02X?}"
        );
    }
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x01, 0xFF, 0x00]).as_deref(),
        Some(&[0x71, 0x01, 0xFF, 0x00, 0x00][..])
    );
    assert_eq!(ecu.started, Some(Rid::EraseMemory));
}

/// Figure 30's checks ahead of the total length — a request too long for the buffer is
/// 0x31 for a routine not available here, 0x33 for a locked one, 0x12 for a reserved
/// `routineControlType` or one the routine does not support, and 0x13 only once all
/// three pass.
#[test]
fn routine_control_a_truncated_request_is_checked_in_figure_30s_order_before_0x13() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for (front, nrc) in [
        (&[0x31, 0x01, 0x12, 0x34][..], 0x31),
        (&[0x31, 0x01, 0xFF, 0x00][..], 0x33),
        (&[0x31, 0x05, 0x02, 0x01][..], 0x12),
        (&[0x31, 0x02, 0x02, 0x01][..], 0x12),
    ] {
        assert_eq!(
            exchange_truncated(&mut ecu, &mut state, front),
            [0x7F, 0x31, nrc],
            "{front:02X?}"
        );
    }
    let mut state = unlocked_level_3(&mut ecu);
    assert_eq!(
        exchange_truncated(&mut ecu, &mut state, &[0x31, 0x01, 0xFF, 0x00, 0x00]),
        [0x7F, 0x31, 0x13]
    );
    assert_eq!(ecu.started, None);
}

/// Figure 30, key 2 — an option record longer than `MAX_OPTION_LEN` is 0x13 and no
/// handler is asked; one of the wrong length for its routine is the handler's 0x13.
#[test]
fn routine_control_a_wrong_option_record_length_is_0x13() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for request in [
        &[0x31, 0x01, 0x02, 0x01, 0x01, 0x02, 0x03, 0x04, 0x05][..],
        &[0x31, 0x01, 0x02, 0x01][..],
        &[0x31, 0x01, 0x02, 0x01, 0x01, 0x02][..],
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, 0x13][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.started, None);
}

/// Figure 30 and clause 14.2.4 — a sub-function the routine does not support is 0x12,
/// from `supports`; what is left is the routine's: 0x31 for an option record it rejects,
/// 0x22 where it cannot run, and 0x24 for results never produced.
#[test]
fn routine_control_the_routine_verdicts_are_its_codes() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for (request, code) in [
        (&[0x31, 0x02, 0x02, 0x01][..], 0x12),
        (&[0x31, 0x01, 0x02, 0x01, 0xFF][..], 0x31),
        (&[0x31, 0x03, 0x02, 0x01][..], 0x24),
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, code][..]),
            "{request:02X?}"
        );
    }
    ecu.refuse = true;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x01, 0x02, 0x01, 0x07]).as_deref(),
        Some(&[0x7F, 0x31, 0x22][..])
    );
    assert_eq!(ecu.started, None);
}

/// Clause 14.2.3, Table 428 — the positive response echoes the `routineControlType` and
/// the identifier, then carries what the routine wrote; the suppress bit silences it.
#[test]
fn routine_control_the_positive_response_echoes_type_and_identifier() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x01, 0x02, 0x01, 0x07]).as_deref(),
        Some(&[0x71, 0x01, 0x02, 0x01, 0x00][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x03, 0x02, 0x01]).as_deref(),
        Some(&[0x71, 0x03, 0x02, 0x01, 0x00, 0x5A][..])
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &[0x31, 0x83, 0x02, 0x01]),
        None
    );
}

/// Clause 14.2.3, Table 428 — the response buffer holds `71`, the echoed type, the
/// identifier, `routineInfo` and the longest `routineStatusRecord`: the self-test's
/// results fill it exactly.
#[test]
fn routine_control_the_response_bound_counts_routine_info() {
    const BOUND: usize = uds_services::__uds_response_bound!(Ecu, RoutineControl);
    assert_eq!(
        BOUND,
        1 + 1 + 2 + 1 + <Rid as RoutineIdentifier>::MAX_STATUS_LEN
    );
    assert_eq!(BOUND, [0x71, 0x03, 0x02, 0x01, 0x00, 0x5A].len());
}

/// Figure 30, "`SubFunction` supported for `routineIdentifier`?" — a reserved
/// `routineControlType` is 0x12, but only after the identifier's 0x31 and 0x33 checks,
/// and ahead of the option record's length.
#[test]
fn routine_control_a_reserved_control_type_is_0x12_after_the_routine_checks() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    for (request, code) in [
        (&[0x31, 0x04, 0x02, 0x01][..], 0x12),
        (
            &[0x31, 0x80, 0x02, 0x01, 0x01, 0x02, 0x03, 0x04, 0x05][..],
            0x12,
        ),
        (&[0x31, 0x7F, 0x12, 0x34][..], 0x31),
        (&[0x31, 0x00, 0xFF, 0x00][..], 0x33),
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, code][..]),
            "{request:02X?}"
        );
    }
    assert_eq!(ecu.started, None);
}

/// Figure 30 — "`SubFunction` supported for `routineIdentifier`?" precedes the total length
/// check: a sub-function the routine does not support is 0x12 even with an option record
/// longer than `MAX_OPTION_LEN`, and a supported one with that record is 0x13.
#[test]
fn routine_control_an_unsupported_sub_function_is_0x12_before_the_length_check() {
    let mut ecu = Ecu::default();
    let mut state = State::INITIAL;
    for (request, code) in [
        (
            &[0x31, 0x02, 0x02, 0x01, 0x01, 0x02, 0x03, 0x04, 0x05][..],
            0x12,
        ),
        (
            &[0x31, 0x03, 0x02, 0x02, 0x01, 0x02, 0x03, 0x04, 0x05][..],
            0x31,
        ),
        (
            &[0x31, 0x01, 0x02, 0x01, 0x01, 0x02, 0x03, 0x04, 0x05][..],
            0x13,
        ),
    ] {
        assert_eq!(
            exchange(&mut ecu, &mut state, request).as_deref(),
            Some(&[0x7F, 0x31, code][..]),
            "{request:02X?}"
        );
    }
}

/// ``UDSSVC_ARCH_0038`` — `on_transition` names the session entered, which its
/// `SessionTransition` cannot: extended to programming and extended to extended are both
/// non-default to non-default, and a `tS3_Server` expiry enters the default session.
#[test]
fn on_transition_is_told_the_session_entered() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(ecu.entered, Some(S::ExtendedDiagnosticSession));
    ecu.session_confirmed(&mut state, S::ProgrammingSession);
    assert_eq!(ecu.entered, Some(S::ProgrammingSession));
    ecu.session_timed_out(&mut state);
    assert_eq!(ecu.entered, Some(S::DefaultSession));
}

/// ISO 14229-1:2020 clause 10.4.1 — "the server shall never send an all zero seed for a
/// given security level that is currently locked", because a client reads a zero seed as
/// "unlocked". The application writes the seed, so a debug build checks it.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "all-zero seed")]
fn security_access_an_all_zero_seed_for_a_locked_level_panics_in_debug() {
    let mut ecu = Ecu {
        seed_fault: SeedFault::Zero,
        ..Ecu::default()
    };
    let mut state = extended(&mut ecu);
    let _ = exchange(&mut ecu, &mut state, &[0x27, 0x01]);
}

/// Annex I Table I.2 transition 9, with clause 8.7.3's `responseTooLong` (0x14) — a
/// `requestSeed` whose response does not fit reaches the tester as a negative response,
/// so no seed awaits a key: the right key is 0x24, and a wrong one is 0x24 without
/// counting an attempt.
#[test]
fn security_access_a_seed_answered_0x14_awaits_no_key() {
    let mut ecu = Ecu::default();
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange_bounded(&mut ecu, &mut state, &[0x27, 0x01], 3),
        [0x7F, 0x27, 0x14]
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &RIGHT_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
    assert_eq!(
        exchange_bounded(&mut ecu, &mut state, &[0x27, 0x01], 3),
        [0x7F, 0x27, 0x14]
    );
    assert_eq!(
        exchange(&mut ecu, &mut state, &WRONG_KEY).as_deref(),
        Some(&[0x7F, 0x27, 0x24][..])
    );
    assert_eq!(ecu.attempts, 0);
}

/// Clause 10.4.1's zero-seed check reads only a seed the sink accepted: a non-zero seed
/// refused part-way is `responseTooLong` (0x14), not a debug panic.
#[test]
fn security_access_a_refused_seed_is_0x14_not_a_zero_seed() {
    let mut ecu = Ecu {
        seed_fault: SeedFault::Split,
        ..Ecu::default()
    };
    let mut state = extended(&mut ecu);
    assert_eq!(
        exchange_bounded(&mut ecu, &mut state, &[0x27, 0x01], 3),
        [0x7F, 0x27, 0x14]
    );
}
