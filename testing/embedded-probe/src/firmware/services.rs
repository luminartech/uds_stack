//! An application implementing every staged service: each handler does what a sensor's
//! would, so that the pipeline's every stage is reachable.

use core::future::{Future, ready};

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, ClearDiagnosticInformation, CommunicationControl, CommunicationControlType,
    CommunicationType, ControlDtcSetting, DataIdentifier, Delay, DiagnosticSessionControl,
    DiagnosticSessionType as S, DtcRecord, DtcReportKind, DtcSettingType, DtcStatusMask,
    EcuReset, KeyVerdict, Levels, ReadDataByIdentifier, ReadDtcInfoReportType,
    ReadDtcInfoSubFunction, ReadDtcInformation, RecordError, ResetType, ResponseSink,
    RoutineControl, RoutineControlSubFunction, RoutineIdentifier, SecurityAccess,
    SecurityLevel, SecurityPolicy, SessionTiming, SessionTransition, Sessions, Sink,
    SubnetNumber, TesterPresent, WriteDataByIdentifier,
};

const VIN: [u8; 17] = *b"WP0ZZZ99ZTS392124";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Did {
    Vin,
    VehicleSpeed,
    Config,
    Mode,
}

impl Did {
    const fn width(self) -> usize {
        match self {
            Self::Vin => VIN.len(),
            Self::Config => 2,
            Self::VehicleSpeed | Self::Mode => 1,
        }
    }
}

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = VIN.len();
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
        let width = self.width();
        let (record, rest) = buf.split_at_checked(width).ok_or(RecordError::Short)?;
        match (self, record) {
            (Self::Mode, [mode]) if *mode > 0x03 => {
                Err(RecordError::Malformed { len: width })
            }
            _ => Ok((record, rest)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rid {
    EraseMemory,
    SelfTest,
}

impl RoutineIdentifier for Rid {
    const MAX_STATUS_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        match self {
            Self::EraseMemory => 0xFF00,
            Self::SelfTest => 0x0201,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xFF00 => Some(Self::EraseMemory),
            0x0201 => Some(Self::SelfTest),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) struct Ecu {
    attempts: u8,
    delay: Delay,
    seeds: u16,
    config: [u8; 2],
    mode: u8,
    started: Option<Rid>,
    communication: Option<CommunicationControlType>,
    dtc_setting: Option<DtcSettingType>,
}

impl Ecu {
    pub(super) const fn new() -> Self {
        Self {
            attempts: 0,
            delay: Delay::Idle,
            seeds: 0,
            config: [0; 2],
            mode: 0,
            started: None,
            communication: None,
            dtc_setting: None,
        }
    }

    fn seed_of(&self, level: SecurityLevel) -> u16 {
        match level.request_seed() {
            0x01 => 0x3657,
            _ => 0x1233_u16.wrapping_add(self.seeds),
        }
    }
}

fn everywhere() -> Access {
    Access::new(Sessions::ALL)
}

fn only_in(session: S) -> Access {
    Access::new(Sessions::of(&[session]))
}

fn by_level_3(access: Access) -> Access {
    match SecurityLevel::from_request_seed(0x03) {
        Some(level) => access.unlocked_by(Levels::NONE.with(level)),
        None => access,
    }
}

impl DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
    }
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    fn leaves_running_software(&self, s: S) -> bool {
        matches!(s, S::ProgrammingSession)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _entered: S, relocked: bool) {
        if relocked {
            self.started = None;
        }
    }
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, kind: ResetType) -> Option<Access> {
        match kind {
            ResetType::HardReset | ResetType::EnableRapidPowerShutDown => {
                Some(everywhere())
            }
            ResetType::SoftReset => Some(by_level_3(everywhere())),
            _ => None,
        }
    }
    fn reset(
        &mut self,
        kind: ResetType,
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        if matches!(kind, ResetType::EnableRapidPowerShutDown) {
            let _ = out.write_all(&[0x0A]);
        }
        ready(Ok(()))
    }
}

impl SecurityAccess for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_SEED_LEN: usize = 2;
    const MAX_KEY_LEN: usize = 2;
    const MAX_RECORD_LEN: usize = 2;
    fn sessions(&self, level: SecurityLevel) -> Option<Sessions> {
        match level.request_seed() {
            0x01 | 0x03 => Some(Sessions::ALL),
            _ => None,
        }
    }
    fn preconditions_met(&self, _l: SecurityLevel) -> bool {
        true
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
    }
    fn seed(
        &mut self,
        level: SecurityLevel,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        if !matches!(record, [] | [0x0E, 0x80]) {
            return ready(Err(Nrc::RequestOutOfRange));
        }
        self.seeds = self.seeds.wrapping_add(1);
        let _ = out.write_all(&self.seed_of(level).to_be_bytes());
        ready(Ok(()))
    }
    fn verify_key(
        &mut self,
        level: SecurityLevel,
        key: &[u8],
    ) -> impl Future<Output = Result<KeyVerdict, Nrc>> {
        ready(Ok(
            if key == self.seed_of(level).wrapping_neg().to_be_bytes() {
                KeyVerdict::Valid
            } else {
                KeyVerdict::Invalid
            },
        ))
    }
}

impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 4;
    fn read(
        &mut self,
        did: Did,
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        let _ = match did {
            Did::Vin => out.write_all(&VIN),
            Did::VehicleSpeed => out.write_all(&[0]),
            Did::Config => out.write_all(&self.config),
            Did::Mode => out.write_all(&[self.mode]),
        };
        ready(Ok(()))
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
    fn write(&mut self, did: Did, record: &[u8]) -> impl Future<Output = Result<(), Nrc>> {
        match (did, record) {
            (Did::Config, [0xFF, 0xFF]) => return ready(Err(Nrc::RequestOutOfRange)),
            (Did::Config, &[high, low]) => self.config = [high, low],
            (Did::Mode, &[mode]) => self.mode = mode,
            _ => {}
        }
        ready(Ok(()))
    }
}

const DTCS: [([u8; 3], u8); 3] = [
    ([0x08, 0x05, 0x11], 0x24),
    ([0x0A, 0x9B, 0x17], 0x26),
    ([0x25, 0x22, 0x1F], 0x2F),
];
const AVAILABILITY: u8 = 0x2F;

impl ReadDtcInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DTCS: usize = DTCS.len();
    const REPORTS: &'static [DtcReportKind] =
        &[DtcReportKind::Count, DtcReportKind::DtcList];
    fn access(&self, report: ReadDtcInfoReportType) -> Option<Access> {
        match u8::from(report) {
            0x01 | 0x02 => Some(everywhere()),
            0x0A => Some(only_in(S::ExtendedDiagnosticSession)),
            _ => None,
        }
    }
    fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
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
            _ => return ready(Err(Nrc::SubFunctionNotSupported)),
        }
        ready(Ok(()))
    }
}

impl ClearDiagnosticInformation for Ecu {
    const MAY_RESPOND_PENDING: bool = true;
    fn clear(
        &mut self,
        group: DtcRecord,
        memory: Option<u8>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        ready(
            if memory.is_some_and(|m| m != 0x00) || group != uds_services::CLEAR_ALL_DTCS {
                Err(Nrc::RequestOutOfRange)
            } else {
                Ok(())
            },
        )
    }
}

impl RoutineControl for Ecu {
    type Rid = Rid;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_OPTION_LEN: usize = 4;
    fn access(&self, routine: Rid) -> Option<Access> {
        match routine {
            Rid::SelfTest => Some(everywhere()),
            Rid::EraseMemory => Some(by_level_3(everywhere())),
        }
    }
    fn supports(&self, routine: Rid, control: RoutineControlSubFunction) -> bool {
        match routine {
            Rid::SelfTest => !matches!(control, RoutineControlSubFunction::StopRoutine),
            Rid::EraseMemory => matches!(control, RoutineControlSubFunction::StartRoutine),
        }
    }
    fn start(
        &mut self,
        routine: Rid,
        record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        if matches!(routine, Rid::SelfTest) && record.len() != 1 {
            return ready(Err(Nrc::IncorrectMessageLengthOrInvalidFormat));
        }
        self.started = Some(routine);
        let _ = out.write_all(&[0x00]);
        ready(Ok(()))
    }
    fn stop(
        &mut self,
        _routine: Rid,
        _record: &[u8],
        _out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        ready(Err(Nrc::SubFunctionNotSupported))
    }
    fn results(
        &mut self,
        routine: Rid,
        _record: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        if self.started != Some(routine) {
            return ready(Err(Nrc::RequestSequenceError));
        }
        let _ = out.write_all(&[0x00, 0x5A]);
        ready(Ok(()))
    }
}

impl CommunicationControl for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, kind: CommunicationControlType) -> Option<Access> {
        match u8::from(kind) {
            0x00 | 0x01 | 0x04 => Some(everywhere()),
            0x03 => Some(by_level_3(everywhere())),
            _ => None,
        }
    }
    fn control(
        &mut self,
        control_type: CommunicationControlType,
        _communication_type: CommunicationType,
        _subnet: SubnetNumber,
        node_id: Option<u16>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        if node_id == Some(0xFFFF) {
            return ready(Err(Nrc::RequestOutOfRange));
        }
        self.communication = Some(control_type);
        ready(Ok(()))
    }
}

impl ControlDtcSetting for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_OPTION_RECORD_LEN: usize = 3;
    fn access(&self, setting: DtcSettingType) -> Option<Access> {
        match u8::from(setting) {
            0x01 | 0x02 => Some(everywhere()),
            _ => None,
        }
    }
    fn control_dtc_setting(
        &mut self,
        setting: DtcSettingType,
        option_record: &[u8],
    ) -> impl Future<Output = Result<(), Nrc>> {
        if option_record.contains(&0xFF) {
            return ready(Err(Nrc::RequestOutOfRange));
        }
        self.dtc_setting = Some(setting);
        ready(Ok(()))
    }
}
