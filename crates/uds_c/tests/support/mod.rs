//! The C side of a server, simulated for host tests, and the server assembled over it as
//! firmware would assemble it.
//!
//! The socket calls run over in-memory buffers with one TCP connection the test drives;
//! each connection the test opens is handed out under a socket index of its own. Two layers
//! serve: a platform layer with the VIN, a slow DID, a routine, a security level and one
//! whose seed handler copies nothing, a DTC memory of [`DTCS`], communication control and
//! download handlers, and records of the transitions it prepares, executes and abandons,
//! and an OEM layer that adds a writable DID, can refuse a reset, and prepares session
//! changes as [`PREPARE`] says. A second server, [`Ecu::start_bare`], serves a layer of one
//! DID and no other handler. Tests share these statics, so each holds [`lock`].

#![allow(unsafe_code, reason = "implements the C functions uds_c calls")]

pub mod sim;

use core::alloc::Layout;
use core::future::Future;
use core::pin::Pin;
use core::ptr::{self, null};
use std::alloc;
use std::boxed::Box;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::vec::Vec;

use embassy_time::{Duration, MockDriver};
use simple_doip::entity::{EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{EntityConfig, TesterAddress};
use simple_doip::{EntityId, LogicalAddress};
use uds_c::doip::{self, RunError};
use uds_c::{
    Firmware, Layers, Policy, TableApp, TableError, TableServer, UdsAccess, UdsDidEntry,
    UdsDtcHooks, UdsLayer, UdsRidEntry, UdsSecurityLevel, UdsTransferHooks,
    UdsTransitionHooks, Validated, uds_record_copy, uds_record_copy_16_be,
};
use uds_on_ip::profile::bench_reloads;
use uds_services::{
    Access, Address, DiagnosticSessionType as Session, DtcStatusMask, P2StarServerMax,
    ResetType, Server, ServerParams, ServiceSet, SessionTiming, Sessions, Stopped,
    UdsTransport, uds_server,
};

/// The tester's logical address.
pub const TESTER: u16 = 0x0E00;
/// The ECU's logical address.
pub const ECU: u16 = 0x4010;
/// The VIN the platform layer's 0xF190 answers.
pub const VIN: [u8; 17] = *b"WVWZZZ1JZXW000001";

const NONE: UdsAccess = UdsAccess {
    sessions: 0,
    security_level: 0,
};

const NO_TRANSFER: UdsTransferHooks = UdsTransferHooks {
    access: NONE,
    begin: None,
    block: None,
    finish: None,
};

const NO_DTC: UdsDtcHooks = UdsDtcHooks {
    get: None,
    snapshot_count: None,
    snapshot_item: None,
    clear: None,
    setting: None,
};

/// What a handler did, in call order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// A layer's execute of a session change.
    Session(&'static str, u8),
    /// A layer's execute of a reset.
    Reset(&'static str, u8),
    /// The slow DID's handler, with its operation status.
    Slow(u8),
    /// The DTC clear handler, with the DTC's index, `UDS_DTC_ALL` for every DTC.
    DtcClear(usize),
    /// The DTC setting handler, with the setting type.
    DtcSetting(u8),
    /// The communication control handler: control type and communication type.
    CommControl(u8, u8),
    /// The download's begin: address, size and data format.
    Begin(u32, u32, u8),
    /// A download block, by its bytes' count and first byte.
    Block(usize, u8),
    /// The download's finish.
    Finish,
    /// A layer's prepare of a session change: operation status, active session and
    /// sub-function.
    Prepare(&'static str, u8, u8, u8),
    /// A layer's abandon of a session change: active session and sub-function.
    Abandon(&'static str, u8, u8),
    /// A layer's abandon of a reset: active session and sub-function.
    AbandonReset(&'static str, u8, u8),
}

/// Every handler call recorded so far.
pub static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());

fn record(call: Call) {
    CALLS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(call);
}

/// The calls recorded since the last take.
pub fn calls() -> Vec<Call> {
    core::mem::take(&mut *CALLS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether the OEM layer refuses a reset: `conditionsNotCorrect` from its check.
pub static REFUSE_RESET: Mutex<bool> = Mutex::new(false);

/// How the OEM layer prepares a session change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prepare {
    /// Done at once.
    Ready,
    /// Response-pending on its first call, then done.
    PendOnce,
    /// Response-pending until canceled.
    Pend,
    /// Refused, `conditionsNotCorrect`.
    Refuse,
}

/// How the OEM layer's next session prepares go.
pub static PREPARE: Mutex<Prepare> = Mutex::new(Prepare::Ready);

fn answer(copied: bool) -> u8 {
    if copied { 0x00 } else { 0x10 }
}

unsafe extern "C" fn read_vin(_op: u8) -> u8 {
    // SAFETY: a static source of its length.
    answer(unsafe { uds_record_copy(0, VIN.as_ptr(), VIN.len()) })
}

/// Pending twice, copying one byte each call, then done.
unsafe extern "C" fn read_slow(op: u8) -> u8 {
    record(Call::Slow(op));
    let calls = CALLS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|c| matches!(c, Call::Slow(_)))
        .count();
    let byte = [u8::try_from(calls).unwrap_or(0xFF)];
    // SAFETY: a local source of its length.
    let copied = unsafe { uds_record_copy(calls - 1, byte.as_ptr(), 1) };
    match (op, calls) {
        (0x02, _) => 0x00,
        (_, 1 | 2) if copied => 0x78,
        _ => answer(copied),
    }
}

/// A status of two bytes, from a routine that declares four.
unsafe extern "C" fn routine_start(_op: u8, option: *const u8, option_len: usize) -> u8 {
    // SAFETY: the server passes `option_len` readable bytes.
    let option = unsafe { core::slice::from_raw_parts(option, option_len) };
    let status = [0xA0, option.first().copied().unwrap_or(0)];
    // SAFETY: a local source of its length.
    answer(unsafe { uds_record_copy(0, status.as_ptr(), status.len()) })
}

unsafe extern "C" fn seed(_op: u8, _option: *const u8, _option_len: usize) -> u8 {
    answer(uds_record_copy_16_be(0, 0x1234))
}

/// A seed handler that answers positive having copied nothing.
unsafe extern "C" fn seed_none(_op: u8, _option: *const u8, _option_len: usize) -> u8 {
    0x00
}

/// The right key is the seed plus one.
unsafe extern "C" fn key(_op: u8, key: *const u8, key_len: usize) -> u8 {
    // SAFETY: the server passes `key_len` readable bytes.
    let key = unsafe { core::slice::from_raw_parts(key, key_len) };
    if key == [0x12, 0x35] { 0x00 } else { 0x35 }
}

unsafe extern "C" fn write_ok(_op: u8, _data: *const u8, _len: usize) -> u8 {
    0x00
}

/// The platform layer's DTC memory: four DTCs and their statuses. The first holds a
/// snapshot record of [`SNAPSHOT`]'s identifiers and sets `warningIndicatorRequested`,
/// which the availability mask leaves out; the second's number carries a fourth byte,
/// which no DTC number has; the third counts a snapshot identifier it cannot give; the
/// fourth holds [`MANY_IDENTIFIERS`] empty identifiers, 0x0200 up, more than the count
/// byte counts.
pub const DTCS: [(u32, u8); 4] = [
    (0xEF_0100, 0x89),
    (0x01EF_0200, 0x50),
    (0xEF_0300, 0x00),
    (0xEF_0400, 0x00),
];

/// The identifiers in the fourth DTC's snapshot record.
pub const MANY_IDENTIFIERS: usize = 256;

/// The first DTC's snapshot record: each identifier and its data.
pub static SNAPSHOT: [(u16, &[u8]); 2] = [(0x0100, &[0xAA]), (0x0101, &[0x01, 0x02])];

unsafe extern "C" fn dtc_get(index: usize, dtc: *mut u32, status: *mut u8) -> bool {
    let Some(&(number, byte)) = DTCS.get(index) else {
        return false;
    };
    // SAFETY: the server passes valid out-pointers (uds_c.h).
    unsafe {
        dtc.write(number);
        status.write(byte);
    }
    true
}

unsafe extern "C" fn dtc_snapshot_count(index: usize) -> usize {
    match index {
        0 => SNAPSHOT.len(),
        2 => 1,
        3 => MANY_IDENTIFIERS,
        _ => 0,
    }
}

unsafe extern "C" fn dtc_snapshot_item(
    index: usize,
    item: usize,
    did: *mut u16,
    data: *mut *const u8,
    len: *mut usize,
) -> bool {
    if index == 3 && item < MANY_IDENTIFIERS {
        let Ok(item) = u16::try_from(item) else {
            return false;
        };
        // SAFETY: the server passes valid out-pointers (uds_c.h); an empty item's data
        // may be NULL.
        unsafe {
            did.write(0x0200 | item);
            data.write(core::ptr::null());
            len.write(0);
        }
        return true;
    }
    let Some(&(id, bytes)) = SNAPSHOT.get(item).filter(|_| index == 0) else {
        return false;
    };
    // SAFETY: the server passes valid out-pointers (uds_c.h); the data is static.
    unsafe {
        did.write(id);
        data.write(bytes.as_ptr());
        len.write(bytes.len());
    }
    true
}

unsafe extern "C" fn dtc_clear(_op: u8, index: usize) -> u8 {
    record(Call::DtcClear(index));
    0x00
}

unsafe extern "C" fn dtc_setting(_op: u8, setting_type: u8) -> u8 {
    record(Call::DtcSetting(setting_type));
    0x00
}

unsafe extern "C" fn comm_control(_op: u8, control_type: u8, communication_type: u8) -> u8 {
    record(Call::CommControl(control_type, communication_type));
    0x00
}

unsafe extern "C" fn begin(_op: u8, address: u32, size: u32, data_format: u8) -> u8 {
    record(Call::Begin(address, size, data_format));
    0x00
}

unsafe extern "C" fn block(_op: u8, data: *const u8, len: usize) -> u8 {
    // SAFETY: the server passes `len` readable bytes.
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    record(Call::Block(len, data.first().copied().unwrap_or(0)));
    0x00
}

unsafe extern "C" fn finish(_op: u8) -> u8 {
    record(Call::Finish);
    0x00
}

unsafe extern "C" fn platform_session(sub_function: u8) {
    record(Call::Session("platform", sub_function));
}

unsafe extern "C" fn platform_reset(sub_function: u8) {
    record(Call::Reset("platform", sub_function));
}

unsafe extern "C" fn oem_session(sub_function: u8) {
    record(Call::Session("oem", sub_function));
}

unsafe extern "C" fn oem_reset(sub_function: u8) {
    record(Call::Reset("oem", sub_function));
}

unsafe extern "C" fn platform_session_prepare(op: u8, active: u8, sub_function: u8) -> u8 {
    record(Call::Prepare("platform", op, active, sub_function));
    0x00
}

unsafe extern "C" fn oem_session_prepare(op: u8, active: u8, sub_function: u8) -> u8 {
    record(Call::Prepare("oem", op, active, sub_function));
    match *PREPARE.lock().unwrap_or_else(PoisonError::into_inner) {
        Prepare::PendOnce if op == 0x00 => 0x78,
        Prepare::Pend if op != 0x02 => 0x78,
        Prepare::Ready | Prepare::PendOnce | Prepare::Pend => 0x00,
        Prepare::Refuse => 0x22,
    }
}

unsafe extern "C" fn platform_session_abandon(active: u8, sub_function: u8) {
    record(Call::Abandon("platform", active, sub_function));
}

unsafe extern "C" fn oem_session_abandon(active: u8, sub_function: u8) {
    record(Call::Abandon("oem", active, sub_function));
}

unsafe extern "C" fn platform_reset_abandon(active: u8, sub_function: u8) {
    record(Call::AbandonReset("platform", active, sub_function));
}

unsafe extern "C" fn oem_reset_abandon(active: u8, sub_function: u8) {
    record(Call::AbandonReset("oem", active, sub_function));
}

unsafe extern "C" fn oem_reset_check(_active: u8, _sub_function: u8) -> u8 {
    if *REFUSE_RESET.lock().unwrap_or_else(PoisonError::into_inner) {
        0x22
    } else {
        0x00
    }
}

/// The DID whose read pends twice.
pub const SLOW_DID: u16 = 0x0100;

static PLATFORM_DIDS: [UdsDidEntry; 2] = [
    UdsDidEntry {
        did: 0xF190,
        read_sessions: 0x07,
        read: Some(read_vin),
        write_access: NONE,
        len: 17,
        write: None,
    },
    UdsDidEntry {
        did: SLOW_DID,
        read_sessions: 0x07,
        read: Some(read_slow),
        write_access: NONE,
        len: 4,
        write: None,
    },
];

static PLATFORM_RIDS: [UdsRidEntry; 1] = [UdsRidEntry {
    rid: 0x0201,
    status_len: 4,
    access: UdsAccess {
        sessions: 0x07,
        security_level: 0,
    },
    start: Some(routine_start),
    stop: None,
    results: None,
}];

static PLATFORM_LEVELS: [UdsSecurityLevel; 2] = [
    UdsSecurityLevel {
        level: 0x01,
        sessions: 0x04,
        attempt_limit: 3,
        seed_len: 2,
        delay_ms: 10_000,
        seed: Some(seed),
        key: Some(key),
    },
    UdsSecurityLevel {
        level: 0x03,
        sessions: 0x04,
        attempt_limit: 3,
        seed_len: 2,
        delay_ms: 10_000,
        seed: Some(seed_none),
        key: Some(key),
    },
];

static OEM_DIDS: [UdsDidEntry; 1] = [UdsDidEntry {
    did: 0x0200,
    read_sessions: 0,
    read: None,
    write_access: UdsAccess {
        sessions: 0x04,
        security_level: 0x01,
    },
    len: 2,
    write: Some(write_ok),
}];

static PLATFORM: UdsLayer = UdsLayer {
    dids: PLATFORM_DIDS.as_ptr(),
    did_count: PLATFORM_DIDS.len(),
    rids: PLATFORM_RIDS.as_ptr(),
    rid_count: PLATFORM_RIDS.len(),
    security_levels: PLATFORM_LEVELS.as_ptr(),
    security_level_count: PLATFORM_LEVELS.len(),
    dtc: UdsDtcHooks {
        get: Some(dtc_get),
        snapshot_count: Some(dtc_snapshot_count),
        snapshot_item: Some(dtc_snapshot_item),
        clear: Some(dtc_clear),
        setting: Some(dtc_setting),
    },
    comm_control: Some(comm_control),
    transfer: UdsTransferHooks {
        access: UdsAccess {
            sessions: 0x02,
            security_level: 0,
        },
        begin: Some(begin),
        block: Some(block),
        finish: Some(finish),
    },
    session: UdsTransitionHooks {
        check: None,
        prepare: Some(platform_session_prepare),
        execute: Some(platform_session),
        abandon: Some(platform_session_abandon),
    },
    reset: UdsTransitionHooks {
        check: None,
        prepare: None,
        execute: Some(platform_reset),
        abandon: Some(platform_reset_abandon),
    },
};

static OEM: UdsLayer = UdsLayer {
    dids: OEM_DIDS.as_ptr(),
    did_count: OEM_DIDS.len(),
    rids: null(),
    rid_count: 0,
    security_levels: null(),
    security_level_count: 0,
    dtc: NO_DTC,
    comm_control: None,
    transfer: NO_TRANSFER,
    session: UdsTransitionHooks {
        check: None,
        prepare: Some(oem_session_prepare),
        execute: Some(oem_session),
        abandon: Some(oem_session_abandon),
    },
    reset: UdsTransitionHooks {
        check: Some(oem_reset_check),
        prepare: None,
        execute: Some(oem_reset),
        abandon: Some(oem_reset_abandon),
    },
};

/// The layers, platform first.
// SAFETY: constant tables, and every test that serves them holds `lock`.
pub static LAYERS: Layers = unsafe { Layers::new(&[&PLATFORM, &OEM]) };

/// The ISO 14229-2 default timing in every session.
const TIMING: SessionTiming = SessionTiming {
    p2_server_max_ms: 50,
    p2_star_server_max: P2StarServerMax::from_tens_of_millis(500),
};

/// A policy as an integrator writes one: three sessions, the programming session
/// entered only from the extended one, and the hard reset and enableRapidPowerShutDown.
#[derive(Debug)]
pub struct Bench;

impl Policy for Bench {
    const LAYERS: &'static Layers = &LAYERS;
    /// Room in the response for the snapshot of [`MANY_IDENTIFIERS`].
    const MAX_DTCS: usize = 160;
    const SESSIONS: Sessions = Sessions::NONE
        .with(Session::DefaultSession)
        .with(Session::ProgrammingSession)
        .with(Session::ExtendedDiagnosticSession);

    fn timing(_session: Session) -> SessionTiming {
        TIMING
    }

    fn supported_from(session: Session, active: Session) -> bool {
        session != Session::ProgrammingSession
            || active == Session::ExtendedDiagnosticSession
    }

    fn reset_access(kind: ResetType) -> Option<Access> {
        matches!(
            kind,
            ResetType::HardReset | ResetType::EnableRapidPowerShutDown
        )
        .then_some(Access::new(Sessions::ALL))
    }
}

/// The application, as firmware names it.
pub type App = TableApp<Bench>;

type Transport = doip::Transport<2048, 1>;

uds_server! {
    App: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = Transport,
    peers = 1,
    services = BenchServices,
    server = BenchServer,
    serves = App::serves,
}

/// [`Bench`], its DTC memory declaring `warningIndicatorRequested` available as well.
#[derive(Debug)]
struct Warning;

impl Policy for Warning {
    const LAYERS: &'static Layers = &LAYERS;
    const MAX_DTCS: usize = Bench::MAX_DTCS;
    const SESSIONS: Sessions = Bench::SESSIONS;
    const DTC_AVAILABILITY: DtcStatusMask = DtcStatusMask::all_flags();

    fn timing(session: Session) -> SessionTiming {
        Bench::timing(session)
    }
}

type WarningApp = TableApp<Warning>;

uds_server! {
    WarningApp: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = Transport,
    peers = 1,
    services = WarningServices,
    server = WarningServer,
    serves = WarningApp::serves,
}

static BARE_DIDS: [UdsDidEntry; 1] = [UdsDidEntry {
    did: 0xF190,
    read_sessions: 0x07,
    read: Some(read_vin),
    write_access: NONE,
    len: 17,
    write: None,
}];

static BARE: UdsLayer = UdsLayer {
    dids: BARE_DIDS.as_ptr(),
    did_count: BARE_DIDS.len(),
    rids: null(),
    rid_count: 0,
    security_levels: null(),
    security_level_count: 0,
    dtc: NO_DTC,
    comm_control: None,
    transfer: NO_TRANSFER,
    session: NO_TRANSITION,
    reset: NO_TRANSITION,
};

const NO_TRANSITION: UdsTransitionHooks = UdsTransitionHooks {
    check: None,
    prepare: None,
    execute: None,
    abandon: None,
};

/// A layer of one DID and no other handler.
// SAFETY: constant tables, and every test that serves them holds `lock`.
static BARE_LAYERS: Layers = unsafe { Layers::new(&[&BARE]) };

/// A policy serving [`BARE_LAYERS`], otherwise [`Bench`]'s.
#[derive(Debug)]
struct Bare;

impl Policy for Bare {
    const LAYERS: &'static Layers = &BARE_LAYERS;
    const SESSIONS: Sessions = Bench::SESSIONS;

    fn timing(session: Session) -> SessionTiming {
        Bench::timing(session)
    }
}

type BareApp = TableApp<Bare>;

uds_server! {
    BareApp: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = Transport,
    peers = 1,
    services = BareServices,
    server = BareServer,
    serves = BareApp::serves,
}

const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 10);

const ENTITY: EntityAddress =
    match EntityAddress::new(LogicalAddress(ECU), LogicalAddress(0xE400)) {
        Ok(address) => address,
        Err(_) => panic!("invalid entity address"),
    };

const TESTER_ADDRESS: TesterAddress = match TesterAddress::new(LogicalAddress(TESTER)) {
    Ok(tester) => tester,
    Err(_) => panic!("invalid tester address"),
};

type Error = <Transport as UdsTransport>::Error;

type Run = Pin<Box<dyn Future<Output = Result<Stopped, Error>>>>;

/// The `validate` of the application assembly `A` serves.
type Validate<A> =
    fn() -> Result<Validated<<Server<A, Transport, 1> as TableServer>::App>, TableError>;

/// A server in a [`Firmware`], armed as firmware arms it, polled as firmware polls it.
pub struct Ecu {
    /// One pass of the firmware's loop.
    poll: Box<dyn FnMut() -> Result<(), RunError<Error>>>,
    /// Frees the firmware.
    free: Option<Box<dyn FnOnce()>>,
}

impl Ecu {
    /// The bench server, with discovery started and its run future armed.
    pub fn start() -> Self {
        Self::serve::<BenchServices>(App::new(), App::validate)
    }

    /// The server of [`Warning`], the bench's layers reporting every status bit, started
    /// as [`Ecu::start`] starts the bench's.
    pub fn start_warning() -> Self {
        Self::serve::<WarningServices>(WarningApp::new(), WarningApp::validate)
    }

    /// The server of [`Bare`], one DID and no other handler, started as [`Ecu::start`]
    /// starts the bench's.
    pub fn start_bare() -> Self {
        Self::serve::<BareServices>(BareApp::new(), BareApp::validate)
    }

    fn serve<A: ServiceSet + 'static>(app: A::App, validate: Validate<A>) -> Self
    where
        Server<A, Transport, 1>: TableServer,
    {
        let raw: *mut Firmware<Server<A, Transport, 1>, Run> =
            Box::into_raw(Box::new(Firmware::new(Server::new(
                app,
                doip::transport(
                    ENTITY,
                    EntityConfig::new([TESTER_ADDRESS]),
                    bench_reloads(),
                ),
                Address(ECU),
                PARAMS,
            ))));
        // SAFETY: from `Box::into_raw` above, freed only by `free`, once nothing polls it.
        let firmware = unsafe { &*raw };
        // SAFETY: every test that serves holds `lock`, so one thread arms and polls each
        // firmware, and `run` keeps the server reference in the future alone.
        let armed = unsafe { firmware.arm(validate().unwrap(), start_discovery, run) };
        assert!(armed);
        // SAFETY: as for `arm`.
        let poll = Box::new(move || unsafe { firmware.poll(run) });
        let layout = Layout::new::<Firmware<Server<A, Transport, 1>, Run>>();
        // SAFETY: from `Box::into_raw` above, with the box's layout; `Ecu` drops `poll`,
        // the last user of the reference, first. Dropped and freed through `raw` rather
        // than a `Box`, whose unique access the run future's drop, reading the server
        // through its own reference, would conflict with.
        let free = Box::new(move || unsafe {
            ptr::drop_in_place(raw);
            alloc::dealloc(raw.cast(), layout);
        });
        Self {
            poll,
            free: Some(free),
        }
    }

    /// One pass of the firmware's loop, which the server must survive.
    pub fn tick(&mut self) {
        let polled = self.poll();
        assert!(polled.is_ok(), "the server stopped: {polled:?}");
    }

    /// One pass of the firmware's loop, with what the poll returned.
    pub fn poll(&mut self) -> Result<(), RunError<Error>> {
        (self.poll)()
    }

    /// Polls, a millisecond apart, until the ECU has sent `count` frames: the frames it
    /// sent on this connection, as (payload type, payload).
    pub fn exchange(&mut self, count: usize) -> Vec<(u16, Vec<u8>)> {
        for _ in 0..1_000 {
            MockDriver::get().advance(Duration::from_millis(1));
            self.tick();
            let frames = sim::sent();
            if frames.len() >= count {
                return frames;
            }
        }
        panic!("the ECU sent {:?}", sim::sent());
    }

    /// A fresh connection with routing activated.
    pub fn activate(&mut self) {
        sim::connect();
        sim::send(&frame(
            0x0005,
            &[&TESTER.to_be_bytes()[..], &[0x00, 0, 0, 0, 0]].concat(),
        ));
        let frames = self.exchange(1);
        assert_eq!(frames[0].0, 0x0006, "routing activation response");
        assert_eq!(frames[0].1[4], 0x10, "routing activated");
    }

    /// Sends `request` and returns the UDS response that answers it, past any
    /// response-pending.
    pub fn request(&mut self, request: &[u8]) -> Vec<u8> {
        let before = sim::sent().len();
        sim::send(&diagnostic(request));
        let mut count = before + 2;
        loop {
            let frames = self.exchange(count);
            let (kind, payload) = &frames[count - 1];
            assert_eq!(
                *kind, 0x8001,
                "a diagnostic message, after its acknowledgment"
            );
            let uds = payload[4..].to_vec();
            if uds.len() == 3 && uds[0] == 0x7F && uds[2] == 0x78 {
                count += 1;
                continue;
            }
            return uds;
        }
    }
}

impl Drop for Ecu {
    fn drop(&mut self) {
        self.poll = Box::new(|| Err(RunError::NotStarted));
        if let Some(free) = self.free.take() {
            free();
        }
    }
}

/// Starts the server's discovery, as firmware does when it arms the server.
fn start_discovery<A: ServiceSet>(server: &mut Server<A, Transport, 1>) {
    let eid = EntityId::new([0x02, 0, 0, 0, 0, 0x01]).unwrap();
    doip::start_discovery(
        server.transport_mut().unwrap(),
        FixedIdentity::new(eid, DiagnosticPowerModeCode::Ready),
    )
    .unwrap();
}

/// The server's run future, holding the server reference alone.
fn run<A: ServiceSet + 'static>(server: &'static mut Server<A, Transport, 1>) -> Run {
    Box::pin(server.run())
}

/// A firmware of the bench server, never armed.
static IDLE: Firmware<BenchServer, Run> = Firmware::new(BenchServer::new(
    App::new(),
    doip::transport(ENTITY, EntityConfig::new([TESTER_ADDRESS]), bench_reloads()),
    Address(ECU),
    PARAMS,
));

/// A poll of [`IDLE`]: the transitions confirmed responses left to make, for [`LAYERS`],
/// then no run future to poll.
pub fn poll_unarmed() -> Result<(), RunError<Error>> {
    // SAFETY: polled under `lock` alone, and never armed.
    unsafe { IDLE.poll(run) }
}

/// An ECU for one test, the shared state reset: holds the lock for the test's run.
pub fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    MockDriver::get().reset();
    let _ = poll_unarmed();
    sim::reset();
    *REFUSE_RESET.lock().unwrap_or_else(PoisonError::into_inner) = false;
    *PREPARE.lock().unwrap_or_else(PoisonError::into_inner) = Prepare::Ready;
    calls();
    guard
}

/// A `DoIP` frame of `payload_type` around `payload`.
pub fn frame(payload_type: u16, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).unwrap();
    let mut f = std::vec![0x02, 0xFD];
    f.extend(payload_type.to_be_bytes());
    f.extend(len.to_be_bytes());
    f.extend(payload);
    f
}

/// A diagnostic message from the tester to the ECU carrying `uds`.
pub fn diagnostic(uds: &[u8]) -> Vec<u8> {
    frame(
        0x8001,
        &[&TESTER.to_be_bytes()[..], &ECU.to_be_bytes()[..], uds].concat(),
    )
}
