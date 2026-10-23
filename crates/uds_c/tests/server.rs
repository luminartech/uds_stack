//! A server assembled over `uds_c` as firmware assembles it, served to a tester over the
//! simulated C side.

#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test harness: the simulated C side, fixed scripts and bounded polls"
)]

extern crate std;

#[allow(dead_code, reason = "each test uses part of the harness")]
mod support;

use core::sync::atomic::Ordering;

use support::sim::{self, Closed};
use support::{App, Call, Ecu, SLOW_DID, VIN, calls, lock, poll_unarmed};
use uds_c::doip::RunError;
use uds_c::{
    ACTIVE_SESSION_DID, AccessOf, Layers, Policy, TableApp, TableError, UdsDidEntry,
    UdsLayer,
};
use uds_services::{DiagnosticSessionType as Session, SessionTiming, Sessions};

/// A hard reset is answered, its connection closed gracefully (ISO 14229-5:2022
/// REQ 7.11), and executed by every layer, the last first, only on the next pass.
#[test]
fn a_hard_reset_executes_after_its_response() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x11, 0x01]), [0x51, 0x01]);
    assert_eq!(sim::closed(), Some(Closed::Graceful), "closed, not reset");
    assert_eq!(calls(), [], "executed on the pass that sent its response");

    ecu.tick();
    assert_eq!(
        calls(),
        [Call::Reset("oem", 0x01), Call::Reset("platform", 0x01)]
    );
}

/// A layer's check refuses a reset with its code, and nothing is executed.
#[test]
fn a_refused_reset_is_answered_with_the_check_code() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    *support::REFUSE_RESET.lock().unwrap() = true;
    assert_eq!(ecu.request(&[0x11, 0x01]), [0x7F, 0x11, 0x22]);
    ecu.tick();
    assert_eq!(calls(), []);
}

/// A response the firmware takes a few bytes at a time, after would-block, arrives whole.
#[test]
fn a_response_written_in_pieces_arrives_whole() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::WRITE_CAP.store(3, Ordering::Relaxed);
    assert_eq!(
        ecu.request(&[0x22, 0xF1, 0x90]),
        [&[0x62, 0xF1, 0x90][..], &VIN].concat()
    );
}

/// A DID read answers exactly the entry's length, what a pending handler copied kept
/// across its calls and zero where nothing was copied.
#[test]
fn a_pending_read_keeps_its_record_across_calls() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    let [hi, lo] = SLOW_DID.to_be_bytes();
    assert_eq!(ecu.request(&[0x22, hi, lo]), [0x62, hi, lo, 1, 2, 3, 0]);
    assert_eq!(
        calls(),
        [Call::Slow(0x00), Call::Slow(0x01), Call::Slow(0x01)]
    );
}

/// A handler still pending when its connection closes is told to cancel.
#[test]
fn a_pending_read_abandoned_is_canceled() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    let [hi, lo] = SLOW_DID.to_be_bytes();
    sim::send(&support::diagnostic(&[0x22, hi, lo]));
    ecu.exchange(2);
    sim::connect();
    for _ in 0..10 {
        ecu.tick();
    }
    let calls = calls();
    assert_eq!(calls.first(), Some(&Call::Slow(0x00)));
    assert_eq!(calls.last(), Some(&Call::Slow(0x02)), "canceled once, last");
    assert_eq!(calls.iter().filter(|&&c| c == Call::Slow(0x02)).count(), 1);
}

/// The server answers the active session itself.
#[test]
fn the_active_session_is_read_without_a_table_entry() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x8B]), [0x7F, 0x22, 0x31]);
}

/// The policy's transition rule: the programming session only from the extended one.
/// Each change entered is executed by every layer on the pass after its response.
#[test]
fn the_programming_session_is_entered_from_the_extended_session_only() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x02]), [0x7F, 0x10, 0x7E]);
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    assert_eq!(
        calls(),
        [Call::Session("oem", 0x03), Call::Session("platform", 0x03)]
    );
    assert_eq!(ecu.request(&[0x10, 0x02])[..2], [0x50, 0x02]);
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x02]);
}

/// A seed answers the level's length; a wrong key is invalidKey, the right one unlocks
/// the writes its level guards.
#[test]
fn a_security_level_unlocks_with_the_handler_verdict() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    assert_eq!(ecu.request(&[0x2E, 0x02, 0x00, 1, 2]), [0x7F, 0x2E, 0x33]);
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x12, 0x34]);
    assert_eq!(ecu.request(&[0x27, 0x02, 0x00, 0x00]), [0x7F, 0x27, 0x35]);
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x12, 0x34]);
    assert_eq!(ecu.request(&[0x27, 0x02, 0x12, 0x35]), [0x67, 0x02]);
    assert_eq!(ecu.request(&[0x2E, 0x02, 0x00, 1, 2]), [0x6E, 0x02, 0x00]);
}

/// A routine answers as many status bytes as its handler copied.
#[test]
fn a_routine_answers_the_status_it_copied() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(
        ecu.request(&[0x31, 0x01, 0x02, 0x01, 0x7E]),
        [0x71, 0x01, 0x02, 0x01, 0xA0, 0x7E]
    );
    assert_eq!(ecu.request(&[0x31, 0x02, 0x02, 0x01]), [0x7F, 0x31, 0x12]);
}

/// A snapshot report reaches the handler with the DTC and record number it names, and a
/// status report with its mask.
#[test]
fn a_dtc_report_carries_its_parameters() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x01, 0x00, 0x01]),
        [0x59, 0x04, 0xEF, 0x01, 0x00, 0x01]
    );
    assert_eq!(calls(), [Call::DtcReport(0x04, 0x00, 0xEF_0100, 0x01)]);
    let _ = ecu.request(&[0x19, 0x02, 0x08]);
    assert_eq!(calls(), [Call::DtcReport(0x02, 0x08, 0, 0)]);
}

/// A server never armed is not polled.
#[test]
fn an_unarmed_server_is_not_started() {
    assert_eq!(poll_unarmed(), Err(RunError::NotStarted));
}

/// The bench layers pass validation.
#[test]
fn the_bench_layers_validate() {
    assert_eq!(App::validate(), Ok(()));
}

unsafe extern "C" fn read_nothing(_op: u8) -> u8 {
    0x00
}

static LONG_DID: [UdsDidEntry; 1] = [readable(0x0300, 65)];

/// A layer serving `dids` alone.
const fn layer_of(dids: &'static [UdsDidEntry]) -> UdsLayer {
    UdsLayer {
        dids: dids.as_ptr(),
        did_count: dids.len(),
        rids: core::ptr::null(),
        rid_count: 0,
        security_levels: core::ptr::null(),
        security_level_count: 0,
        dtc: uds_c::UdsDtcHooks {
            report: None,
            clear: None,
            setting: None,
        },
        comm_control: None,
        transfer: uds_c::UdsTransferHooks {
            access: uds_c::UdsAccess {
                sessions: 0,
                security_level: 0,
            },
            begin: None,
            block: None,
            finish: None,
        },
        session: uds_c::UdsTransitionHooks {
            check: None,
            prepare: None,
            execute: None,
        },
        reset: uds_c::UdsTransitionHooks {
            check: None,
            prepare: None,
            execute: None,
        },
    }
}

/// A readable DID `did` of `len` bytes.
const fn readable(did: u16, len: u16) -> UdsDidEntry {
    UdsDidEntry {
        did,
        read_sessions: 0x07,
        read: Some(read_nothing),
        write_access: uds_c::UdsAccess {
            sessions: 0,
            security_level: 0,
        },
        len,
        write: None,
    }
}

/// A policy serving `$layers`, its tables only validated, never served.
macro_rules! validated_only {
    ($policy:ident, $($layer:ident),+) => {
        #[derive(Debug)]
        struct $policy;

        impl Policy for $policy {
            const LAYERS: &'static Layers = {
                // SAFETY: constant tables; only validated, never served.
                static LAYERS: Layers = unsafe { Layers::new(&[$(&$layer),+]) };
                &LAYERS
            };
            const SESSIONS: Sessions = Sessions::ALL;

            fn timing(_session: Session) -> SessionTiming {
                SessionTiming {
                    p2_server_max_ms: 50,
                    p2_star_server_max: uds_services::P2StarServerMax::from_tens_of_millis(500),
                }
            }
        }
    };
}

static LONG: UdsLayer = layer_of(&LONG_DID);
validated_only!(Short, LONG);

/// A DID longer than the policy allows is refused at start-up.
#[test]
fn a_record_past_the_policy_limit_is_refused() {
    assert_eq!(
        TableApp::<Short>::validate(),
        Err(TableError::DidTooLong { did: 0x0300 })
    );
}

/// A zero-filled entry, as a table declared longer than its initializer ends with, is
/// refused at start-up.
#[test]
fn an_entry_without_a_handler_is_refused() {
    static BLANK_DID: [UdsDidEntry; 2] = [
        readable(0x0300, 4),
        UdsDidEntry {
            read: None,
            ..readable(0x0000, 0)
        },
    ];
    static BLANK: UdsLayer = layer_of(&BLANK_DID);
    validated_only!(Blank, BLANK);
    assert_eq!(
        TableApp::<Blank>::validate(),
        Err(TableError::NoHandler(AccessOf::Did(0x0000)))
    );
}

/// A DID a later layer lists again, which it could never serve, is refused at start-up.
#[test]
fn a_did_listed_twice_is_refused() {
    static FIRST_DID: [UdsDidEntry; 1] = [readable(0x0300, 4)];
    static AGAIN_DID: [UdsDidEntry; 1] = [readable(0x0300, 8)];
    static FIRST: UdsLayer = layer_of(&FIRST_DID);
    static AGAIN: UdsLayer = layer_of(&AGAIN_DID);
    validated_only!(Twice, FIRST, AGAIN);
    assert_eq!(
        TableApp::<Twice>::validate(),
        Err(TableError::Duplicate(AccessOf::Did(0x0300)))
    );
}

/// A table entry for the DID the server answers itself is refused at start-up.
#[test]
fn the_active_session_did_in_a_table_is_refused() {
    static SESSION_DID: [UdsDidEntry; 1] = [readable(ACTIVE_SESSION_DID, 1)];
    static SESSION: UdsLayer = layer_of(&SESSION_DID);
    validated_only!(Shadow, SESSION);
    assert_eq!(
        TableApp::<Shadow>::validate(),
        Err(TableError::Duplicate(AccessOf::Did(ACTIVE_SESSION_DID)))
    );
}
