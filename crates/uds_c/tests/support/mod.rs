//! The C side of a server, simulated for host tests, and the server assembled over it as
//! firmware would assemble it.
//!
//! The socket calls run over in-memory buffers with one TCP connection the test drives;
//! each connection the test opens is handed out under a socket index of its own. Two
//! layers serve: a platform layer with the VIN, a slow DID, a routine, a security level,
//! the DTC, communication control and download handlers, and records of the transitions
//! it executes, and an OEM layer that adds a writable DID, can refuse a reset, and
//! prepares session changes as [`PREPARE`] says. A second server, [`Ecu::start_bare`],
//! serves a layer of one DID and no other handler. Tests share these statics, so each
//! holds [`lock`].

#![allow(unsafe_code, reason = "implements the C functions uds_c calls")]

pub mod sim;

use core::future::Future;
use core::pin::Pin;
use core::ptr::null;
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
    Layers, Policy, TableApp, UdsAccess, UdsDidEntry, UdsDtcHooks, UdsLayer, UdsRidEntry,
    UdsSecurityLevel, UdsTransferHooks, UdsTransitionHooks, execute_deferred,
    uds_record_copy, uds_record_copy_16_be,
};
use uds_on_ip::profile::bench_reloads;
use uds_services::{
    Address, DiagnosticSessionType as Session, P2StarServerMax, Server, ServerParams,
    ServiceSet, SessionTiming, Sessions, Stopped, UdsTransport, uds_server,
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
    report: None,
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
    /// The DTC report handler: report type, status mask, DTC and record number.
    DtcReport(u8, u8, u32, u8),
    /// The DTC clear handler, with the group.
    DtcClear(u32),
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
    /// The OEM layer's prepare of a session change: operation status and sub-function.
    Prepare(u8, u8),
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

/// The right key is the seed plus one.
unsafe extern "C" fn key(_op: u8, key: *const u8, key_len: usize) -> u8 {
    // SAFETY: the server passes `key_len` readable bytes.
    let key = unsafe { core::slice::from_raw_parts(key, key_len) };
    if key == [0x12, 0x35] { 0x00 } else { 0x35 }
}

unsafe extern "C" fn write_ok(_op: u8, _data: *const u8, _len: usize) -> u8 {
    0x00
}

/// Echoes the DTC a snapshot request names, then one record byte.
unsafe extern "C" fn dtc_report(
    _op: u8,
    report_type: u8,
    mask: u8,
    dtc: u32,
    record_number: u8,
) -> u8 {
    record(Call::DtcReport(report_type, mask, dtc, record_number));
    let [_, high, mid, low] = dtc.to_be_bytes();
    let report = [high, mid, low, record_number];
    // SAFETY: a local source of its length.
    answer(unsafe { uds_record_copy(0, report.as_ptr(), report.len()) })
}

unsafe extern "C" fn dtc_clear(_op: u8, group: u32) -> u8 {
    record(Call::DtcClear(group));
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

unsafe extern "C" fn oem_session_prepare(op: u8, sub_function: u8) -> u8 {
    record(Call::Prepare(op, sub_function));
    match *PREPARE.lock().unwrap_or_else(PoisonError::into_inner) {
        Prepare::PendOnce if op == 0x00 => 0x78,
        Prepare::Ready | Prepare::PendOnce => 0x00,
        Prepare::Refuse => 0x22,
    }
}

unsafe extern "C" fn oem_reset_check(_sub_function: u8) -> u8 {
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

static PLATFORM_LEVELS: [UdsSecurityLevel; 1] = [UdsSecurityLevel {
    level: 0x01,
    sessions: 0x04,
    attempt_limit: 3,
    seed_len: 2,
    delay_ms: 10_000,
    seed: Some(seed),
    key: Some(key),
}];

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
        report: Some(dtc_report),
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
        prepare: None,
        execute: Some(platform_session),
    },
    reset: UdsTransitionHooks {
        check: None,
        prepare: None,
        execute: Some(platform_reset),
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
    },
    reset: UdsTransitionHooks {
        check: Some(oem_reset_check),
        prepare: None,
        execute: Some(oem_reset),
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
/// entered only from the extended one.
#[derive(Debug)]
pub struct Bench;

impl Policy for Bench {
    const LAYERS: &'static Layers = &LAYERS;
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

type Run =
    Pin<Box<dyn Future<Output = Result<Stopped, <Transport as UdsTransport>::Error>>>>;

/// A server armed as firmware arms it, polled as firmware polls it.
pub struct Ecu {
    layers: &'static Layers,
    run: Option<Run>,
    /// The next run future, borrowing the server.
    rearm: Box<dyn Fn() -> Run>,
    /// Frees the server, once no run future borrows it.
    free: Option<Box<dyn FnOnce()>>,
}

impl Ecu {
    /// The bench server, with discovery started and its run future armed.
    pub fn start() -> Self {
        assert_eq!(App::validate(), Ok(()));
        Self::serve::<BenchServices>(App::new(), &LAYERS)
    }

    /// The server of [`Bare`], one DID and no other handler, started as [`Ecu::start`]
    /// starts the bench's.
    pub fn start_bare() -> Self {
        assert_eq!(BareApp::validate(), Ok(()));
        Self::serve::<BareServices>(BareApp::new(), &BARE_LAYERS)
    }

    fn serve<A: ServiceSet + 'static>(app: A::App, layers: &'static Layers) -> Self {
        let server: *mut Server<A, Transport, 1> = Box::into_raw(Box::new(Server::new(
            app,
            doip::transport(ENTITY, EntityConfig::new([TESTER_ADDRESS]), bench_reloads()),
            Address(ECU),
            PARAMS,
        )));
        // SAFETY: just allocated, and nothing else refers to it yet.
        let transport = unsafe { &mut *server }.transport_mut();
        let eid = EntityId::new([0x02, 0, 0, 0, 0, 0x01]).unwrap();
        doip::start_discovery(
            transport,
            FixedIdentity::new(eid, DiagnosticPowerModeCode::Ready),
        );
        // SAFETY: the server lives until `Ecu` is dropped, which drops the run future
        // first, and `poll_run` drops a finished future before arming the next, so no
        // two run futures borrow it at once.
        let rearm = Box::new(move || -> Run { Box::pin(unsafe { &mut *server }.run()) });
        // SAFETY: from `Box::into_raw` above, called once no run future borrows it.
        let free = Box::new(move || drop(unsafe { Box::from_raw(server) }));
        Self {
            layers,
            run: Some(rearm()),
            rearm,
            free: Some(free),
        }
    }

    /// One pass of the firmware's loop: deferred transitions, then one poll, which the
    /// server must survive.
    pub fn tick(&mut self) {
        let polled = self.poll();
        assert!(polled.is_ok(), "the server stopped: {polled:?}");
    }

    /// One pass of the firmware's loop, with what the poll returned.
    pub fn poll(&mut self) -> Result<(), RunError<<Transport as UdsTransport>::Error>> {
        execute_deferred(self.layers);
        doip::poll_run(Pin::new(&mut self.run), &self.rearm)
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
        self.run = None;
        if let Some(free) = self.free.take() {
            free();
        }
    }
}

/// An ECU for one test, the shared state reset: holds the lock for the test's run.
pub fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    MockDriver::get().reset();
    execute_deferred(&LAYERS);
    sim::reset();
    *REFUSE_RESET.lock().unwrap_or_else(PoisonError::into_inner) = false;
    *PREPARE.lock().unwrap_or_else(PoisonError::into_inner) = Prepare::Ready;
    calls();
    guard
}

/// The poll of a server never armed.
pub fn poll_unarmed() -> Result<(), RunError<()>> {
    let mut run: Option<core::future::Ready<Result<Stopped, ()>>> = None;
    doip::poll_run(Pin::new(&mut run), || core::future::ready(Err(())))
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
