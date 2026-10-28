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

use embassy_time::{Duration, MockDriver};
use support::sim::{self, Closed, Fail};
use support::{App, Call, Ecu, Prepare, SLOW_DID, VIN, calls, lock, poll_unarmed};
use uds_c::doip::RunError;
use uds_c::{
    ACTIVE_SESSION_DID, AccessOf, Layers, Policy, TableApp, TableError, UdsAccess,
    UdsDidEntry, UdsLayer, UdsSecurityLevel,
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
        [
            Call::Reset("oem", 0x01),
            Call::Reset("platform", 0x01),
            Call::DtcSetting(0x01),
            Call::CommControl(0x00, 0x03)
        ],
        "a reset whose execute returns leaves the default session, which resumes both"
    );
}

/// A reset whose response the stack has taken but not transmitted is neither confirmed
/// nor executed, and its connection stays open; once the stack transmits the response,
/// the connection closes and the reset executes on the next pass.
#[test]
fn a_reset_executes_only_once_its_response_is_transmitted() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::hold_transmission(true);
    assert_eq!(ecu.request(&[0x11, 0x01]), [0x51, 0x01]);
    for _ in 0..100 {
        MockDriver::get().advance(Duration::from_millis(1));
        ecu.tick();
    }
    assert_eq!(
        sim::closed(),
        None,
        "not closed before the response is transmitted"
    );
    assert_eq!(
        calls(),
        [],
        "not executed before the response is transmitted"
    );

    sim::hold_transmission(false);
    MockDriver::get().advance(Duration::from_millis(1));
    ecu.tick();
    assert_eq!(sim::closed(), Some(Closed::Graceful));
    assert_eq!(
        calls(),
        [],
        "executed on the pass that confirmed its response"
    );

    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Reset("oem", 0x01),
            Call::Reset("platform", 0x01),
            Call::DtcSetting(0x01),
            Call::CommControl(0x00, 0x03)
        ]
    );
}

/// enableRapidPowerShutDown keeps the server running: its response carries the
/// powerDownTime and closes the connection, as every reset's does (ISO 14229-5:2022
/// REQ 7.11), every layer executes it on the next pass, and the session, DTC setting and
/// communication control are kept for the next connection.
#[test]
fn a_rapid_power_shutdown_keeps_the_server_running() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    calls();
    assert_eq!(ecu.request(&[0x11, 0x04]), [0x51, 0x04, 0xFF]);
    assert_eq!(sim::closed(), Some(Closed::Graceful));
    ecu.tick();
    assert_eq!(
        calls(),
        [Call::Reset("oem", 0x04), Call::Reset("platform", 0x04)]
    );
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x03]);
}

/// A hard reset from the extended session is executed first, then the default session it
/// returns to, and DTC setting and communication control resume once.
#[test]
fn a_reset_from_another_session_executes_before_the_default_session() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    assert_eq!(ecu.request(&[0x85, 0x02]), [0xC5, 0x02]);
    calls();
    assert_eq!(ecu.request(&[0x11, 0x01]), [0x51, 0x01]);
    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Reset("oem", 0x01),
            Call::Reset("platform", 0x01),
            Call::Session("oem", 0x01),
            Call::Session("platform", 0x01),
            Call::DtcSetting(0x01),
            Call::CommControl(0x00, 0x03)
        ]
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
/// Each change entered is prepared before its response and executed by every layer on
/// the pass after it.
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
        [
            Call::Prepare("platform", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x00, 0x01, 0x03),
            Call::Session("oem", 0x03),
            Call::Session("platform", 0x03)
        ]
    );
    assert_eq!(ecu.request(&[0x10, 0x02])[..2], [0x50, 0x02]);
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x02]);
}

/// A seed answers the level's length; a wrong key is invalidKey, the right one unlocks
/// the writes its level guards, and the level's zero seed, unlocked, has its length too.
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
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x00, 0x00]);
}

/// A seed handler that leaves the seed all zero, which would read as already unlocked, is
/// answered conditionsNotCorrect.
#[test]
fn a_seed_left_all_zero_is_refused() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    assert_eq!(ecu.request(&[0x27, 0x03]), [0x7F, 0x27, 0x22]);
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

/// The server encodes each report from the layer's DTC memory, against the default
/// availability mask 0x7F: the count and the list of DTCs a status mask matches, every
/// DTC, the DTCs holding a snapshot, and one DTC's snapshot record. A DTC number's
/// fourth byte, and a status bit outside the mask, are not reported.
#[test]
fn the_dtc_reports_are_encoded_from_the_memory() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(
        ecu.request(&[0x19, 0x01, 0x08]),
        [0x59, 0x01, 0x7F, 0x01, 0x00, 0x01]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x02, 0x08]),
        [0x59, 0x02, 0x7F, 0xEF, 0x01, 0x00, 0x09]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x01, 0x80]),
        [0x59, 0x01, 0x7F, 0x01, 0x00, 0x00]
    );
    assert_eq!(ecu.request(&[0x19, 0x02, 0x80]), [0x59, 0x02, 0x7F]);
    assert_eq!(
        ecu.request(&[0x19, 0x0A]),
        [
            0x59, 0x0A, 0x7F, 0xEF, 0x01, 0x00, 0x09, 0xEF, 0x02, 0x00, 0x50, 0xEF, 0x03,
            0x00, 0x00, 0xEF, 0x04, 0x00, 0x00
        ]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x03]),
        [
            0x59, 0x03, 0xEF, 0x01, 0x00, 0x01, 0xEF, 0x03, 0x00, 0x01, 0xEF, 0x04, 0x00,
            0x01
        ]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x01, 0x00, 0x01]),
        [
            0x59, 0x04, 0xEF, 0x01, 0x00, 0x09, 0x01, 0x02, 0x01, 0x00, 0xAA, 0x01, 0x01,
            0x01, 0x02
        ]
    );
    assert_eq!(calls(), []);
}

/// A policy that declares `warningIndicatorRequested` available reports it: the mask
/// 0x80 finds the DTC that sets it, whose status is reported whole.
#[test]
fn a_declared_availability_mask_reports_its_bits() {
    let _lock = lock();
    let mut ecu = Ecu::start_warning();
    ecu.activate();
    assert_eq!(
        ecu.request(&[0x19, 0x01, 0x80]),
        [0x59, 0x01, 0xFF, 0x01, 0x00, 0x01]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x02, 0x80]),
        [0x59, 0x02, 0xFF, 0xEF, 0x01, 0x00, 0x89]
    );
}

/// A snapshot request for a DTC without one answers the DTC and its status alone; a DTC
/// the memory does not hold, or a record number other than its one record or all of
/// them, is requestOutOfRange; a record the memory cannot give in full is generalReject,
/// not a response whose identifier count overstates it; a record of more identifiers
/// than the count byte counts gives the count 0x00, and an empty item its identifier
/// alone.
#[test]
fn a_snapshot_request_names_a_held_dtc_and_record() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x02, 0x00, 0xFF]),
        [0x59, 0x04, 0xEF, 0x02, 0x00, 0x50]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x05, 0x00, 0x01]),
        [0x7F, 0x19, 0x31]
    );
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x03, 0x00, 0x01]),
        [0x7F, 0x19, 0x10]
    );
    let many = ecu.request(&[0x19, 0x04, 0xEF, 0x04, 0x00, 0x01]);
    assert_eq!(many[..8], [0x59, 0x04, 0xEF, 0x04, 0x00, 0x00, 0x01, 0x00]);
    let identifiers: Vec<u8> = (0x0200..0x0300_u16).flat_map(u16::to_be_bytes).collect();
    assert_eq!(many[8..], identifiers[..]);
    assert_eq!(
        ecu.request(&[0x19, 0x04, 0xEF, 0x01, 0x00, 0x02]),
        [0x7F, 0x19, 0x31]
    );
}

/// A session change a layer's prepare refuses is answered with its code, neither entered
/// nor executed, and abandoned by the layer that prepared it first.
#[test]
fn a_refused_prepare_refuses_the_session_change() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    *support::PREPARE.lock().unwrap() = Prepare::Refuse;
    assert_eq!(ecu.request(&[0x10, 0x03]), [0x7F, 0x10, 0x22]);
    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Prepare("platform", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x00, 0x01, 0x03),
            Call::Abandon("platform", 0x01, 0x03)
        ]
    );
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// A prepare that answers pending is called again until it settles, then the change is
/// answered and executed.
#[test]
fn a_pending_prepare_is_called_until_it_settles() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    *support::PREPARE.lock().unwrap() = Prepare::PendOnce;
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Prepare("platform", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x01, 0x01, 0x03),
            Call::Session("oem", 0x03),
            Call::Session("platform", 0x03)
        ]
    );
}

/// A session change still being prepared when its connection closes is canceled in the
/// layer preparing it, then abandoned by the layer that prepared it first.
#[test]
fn a_prepare_canceled_abandons_the_layers_prepared() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    *support::PREPARE.lock().unwrap() = Prepare::Pend;
    sim::send(&support::diagnostic(&[0x10, 0x03]));
    ecu.exchange(2);
    sim::connect();
    for _ in 0..10 {
        ecu.tick();
    }
    let calls = calls();
    assert_eq!(
        calls[calls.len() - 2..],
        [
            Call::Prepare("oem", 0x02, 0x01, 0x03),
            Call::Abandon("platform", 0x01, 0x03)
        ]
    );
    assert!(!calls.iter().any(|c| matches!(c, Call::Session(..))));
}

/// A session change whose positive response cannot be sent is abandoned by every layer,
/// the last first, and not entered.
#[test]
fn a_session_change_left_unanswered_is_abandoned() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::fail(Fail::Write);
    sim::send(&support::diagnostic(&[0x10, 0x03]));
    for _ in 0..10 {
        ecu.tick();
    }
    assert_eq!(
        calls(),
        [
            Call::Prepare("platform", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x00, 0x01, 0x03),
            Call::Abandon("oem", 0x01, 0x03),
            Call::Abandon("platform", 0x01, 0x03)
        ]
    );
    sim::fail(Fail::None);
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// A reset whose positive response cannot be sent is abandoned by every layer, the last
/// first, and not executed.
#[test]
fn a_reset_left_unanswered_is_abandoned() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::fail(Fail::Write);
    sim::send(&support::diagnostic(&[0x11, 0x01]));
    for _ in 0..10 {
        ecu.tick();
    }
    assert_eq!(
        calls(),
        [
            Call::AbandonReset("oem", 0x01, 0x01),
            Call::AbandonReset("platform", 0x01, 0x01)
        ]
    );
}

/// When `tS3_Server` returns the server to the default session, every layer executes it,
/// with no request or response, and DTC setting and communication control resume.
#[test]
fn the_default_session_is_executed_on_s3_expiry() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    calls();
    MockDriver::get().advance(Duration::from_millis(5_001));
    ecu.tick();
    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Session("oem", 0x01),
            Call::Session("platform", 0x01),
            Call::DtcSetting(0x01),
            Call::CommControl(0x00, 0x03)
        ]
    );
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// Clause 10.2 Figure 7: entering a session where `ControlDTCSetting` and
/// `CommunicationControl` are served, the extended or the programming one, leaves their
/// state as it is; entering the default session, where they are not, turns DTC status
/// updates on and enables every communication type.
#[test]
fn dtc_setting_and_communication_control_resume_in_the_default_session() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    assert_eq!(ecu.request(&[0x85, 0x02]), [0xC5, 0x02]);
    calls();
    for session in [0x03, 0x02] {
        assert_eq!(ecu.request(&[0x10, session])[..2], [0x50, session]);
        ecu.tick();
        assert_eq!(
            calls(),
            [
                Call::Prepare("platform", 0x00, 0x03, session),
                Call::Prepare("oem", 0x00, 0x03, session),
                Call::Session("oem", session),
                Call::Session("platform", session)
            ]
        );
    }
    assert_eq!(ecu.request(&[0x10, 0x01])[..2], [0x50, 0x01]);
    ecu.tick();
    assert_eq!(
        calls(),
        [
            Call::Prepare("platform", 0x00, 0x02, 0x01),
            Call::Prepare("oem", 0x00, 0x02, 0x01),
            Call::Session("oem", 0x01),
            Call::Session("platform", 0x01),
            Call::DtcSetting(0x01),
            Call::CommControl(0x00, 0x03)
        ]
    );
}

/// After `attempt_limit` wrong keys the level is locked for its delay: the last wrong key
/// is exceededNumberOfAttempts, a seed meanwhile requiredTimeDelayNotExpired, and once
/// the delay expires, by when `tS3_Server` has ended the extended session, the level
/// unlocks again.
#[test]
fn a_security_level_locks_out_until_its_delay_expires() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    for _ in 0..2 {
        assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x12, 0x34]);
        assert_eq!(ecu.request(&[0x27, 0x02, 0x00, 0x00]), [0x7F, 0x27, 0x35]);
    }
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x12, 0x34]);
    assert_eq!(ecu.request(&[0x27, 0x02, 0x00, 0x00]), [0x7F, 0x27, 0x36]);
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x7F, 0x27, 0x37]);

    MockDriver::get().advance(Duration::from_millis(10_000));
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    assert_eq!(ecu.request(&[0x27, 0x01]), [0x67, 0x01, 0x12, 0x34]);
    assert_eq!(ecu.request(&[0x27, 0x02, 0x12, 0x35]), [0x67, 0x02]);
}

/// `ClearDiagnosticInformation` clears every DTC, or the one its group names, by index;
/// a DTC the memory does not hold, or a `memorySelection`, which the handler has no
/// parameter for, is requestOutOfRange without calling it.
#[test]
fn a_clear_reaches_its_handler_by_index() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x14, 0xFF, 0xFF, 0xFF]), [0x54]);
    assert_eq!(calls(), [Call::DtcClear(uds_c::UDS_DTC_ALL)]);
    assert_eq!(ecu.request(&[0x14, 0xEF, 0x02, 0x00]), [0x54]);
    assert_eq!(calls(), [Call::DtcClear(1)]);
    assert_eq!(ecu.request(&[0x14, 0xEF, 0x05, 0x00]), [0x7F, 0x14, 0x31]);
    assert_eq!(calls(), []);
    assert_eq!(
        ecu.request(&[0x14, 0xFF, 0xFF, 0xFF, 0x01]),
        [0x7F, 0x14, 0x31]
    );
    assert_eq!(calls(), []);
}

/// `ControlDTCSetting` and `CommunicationControl` reach their handlers outside the default
/// session, with the sub-function and its parameter. The one network and DTC group are
/// all there is: a specific subnet is out of range and an option record is refused,
/// neither reaching its handler.
#[test]
fn dtc_setting_and_communication_control_reach_their_handlers() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x85, 0x02]), [0x7F, 0x85, 0x7F]);
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    ecu.tick();
    calls();
    assert_eq!(ecu.request(&[0x85, 0x02]), [0xC5, 0x02]);
    assert_eq!(ecu.request(&[0x85, 0x02, 0x01]), [0x7F, 0x85, 0x13]);
    assert_eq!(ecu.request(&[0x28, 0x03, 0x01]), [0x68, 0x03]);
    assert_eq!(ecu.request(&[0x28, 0x01, 0xF1]), [0x68, 0x01]);
    assert_eq!(ecu.request(&[0x28, 0x01, 0x51]), [0x7F, 0x28, 0x31]);
    assert_eq!(
        ecu.request(&[0x28, 0x04, 0x01, 0x00, 0x01]),
        [0x7F, 0x28, 0x12]
    );
    assert_eq!(
        calls(),
        [
            Call::DtcSetting(0x02),
            Call::CommControl(0x03, 0x01),
            Call::CommControl(0x01, 0x01)
        ]
    );
}

/// A download in the programming session: begun with its address, size and format,
/// one block, then finished.
#[test]
fn a_download_reaches_its_handlers() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    assert_eq!(ecu.request(&[0x10, 0x03])[..2], [0x50, 0x03]);
    assert_eq!(ecu.request(&[0x10, 0x02])[..2], [0x50, 0x02]);
    ecu.tick();
    calls();
    let begin = [
        0x34, 0x00, 0x44, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
    ];
    assert_eq!(ecu.request(&begin), [0x74, 0x20, 0x04, 0x02]);
    assert_eq!(ecu.request(&[0x36, 0x01, 0xA5, 1, 2, 3]), [0x76, 0x01]);
    assert_eq!(ecu.request(&[0x37]), [0x77]);
    assert_eq!(
        calls(),
        [
            Call::Begin(0x0008_0000, 4, 0x00),
            Call::Block(4, 0xA5),
            Call::Finish
        ]
    );
}

/// A service no layer has a handler for is serviceNotSupported, ahead of Table 23's
/// refusal in the default session.
#[test]
fn a_service_no_layer_serves_is_not_supported() {
    let _lock = lock();
    let mut ecu = Ecu::start_bare();
    ecu.activate();
    assert_eq!(ecu.request(&[0x14, 0xFF, 0xFF, 0xFF]), [0x7F, 0x14, 0x11]);
    assert_eq!(ecu.request(&[0x19, 0x02, 0xFF]), [0x7F, 0x19, 0x11]);
    assert_eq!(ecu.request(&[0x28, 0x00, 0x01]), [0x7F, 0x28, 0x11]);
    assert_eq!(ecu.request(&[0x85, 0x01]), [0x7F, 0x85, 0x11]);
    let begin = [
        0x34, 0x00, 0x44, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
    ];
    assert_eq!(ecu.request(&begin), [0x7F, 0x34, 0x11]);
    assert_eq!(ecu.request(&[0x36, 0x01, 0xA5]), [0x7F, 0x36, 0x11]);
    assert_eq!(ecu.request(&[0x37]), [0x7F, 0x37, 0x11]);
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x90])[..3], [0x62, 0xF1, 0x90]);
}

/// A connection whose read fails is reset and released, and the server serves the next.
#[test]
fn a_failed_read_drops_its_connection() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::fail(Fail::Read);
    sim::send(&support::diagnostic(&[0x22, 0xF1, 0x90]));
    for _ in 0..10 {
        ecu.tick();
    }
    assert_eq!(sim::closed(), Some(Closed::Reset));
    sim::fail(Fail::None);
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// A connection whose write fails is reset and released, and the server serves the next.
#[test]
fn a_failed_write_drops_its_connection() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::fail(Fail::Write);
    sim::send(&support::diagnostic(&[0x22, 0xF1, 0x90]));
    for _ in 0..10 {
        ecu.tick();
    }
    assert_eq!(sim::closed(), Some(Closed::Reset));
    sim::fail(Fail::None);
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// A failed accept between requests interrupts the server, which is armed again at once
/// and serves on.
#[test]
fn a_failed_accept_between_requests_interrupts_the_server() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    sim::fail(Fail::Accept);
    sim::connect();
    assert!(matches!(ecu.poll(), Err(RunError::Interrupted(_))));
    sim::fail(Fail::None);
    assert!(!ecu.arm(), "armed again already");
    ecu.activate();
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x01]);
}

/// A failed accept between requests while a session change's positive response is still
/// being written keeps the change: the tester receives the whole response, the layers
/// execute the change once it is transmitted, nothing is abandoned, and the server is in
/// the session the response announced.
#[test]
fn a_failed_accept_while_a_session_change_is_written_keeps_the_change() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    sim::WRITE_CAP.store(4, Ordering::Relaxed);
    sim::send(&support::diagnostic(&[0x10, 0x03]));
    let mut seen = Vec::new();
    for _ in 0..100 {
        if seen.len() >= 2 {
            break;
        }
        ecu.tick();
        seen.extend(calls());
    }
    assert_eq!(
        seen,
        [
            Call::Prepare("platform", 0x00, 0x01, 0x03),
            Call::Prepare("oem", 0x00, 0x01, 0x03)
        ]
    );
    sim::fail(Fail::Accept);
    assert!(matches!(ecu.poll(), Err(RunError::Interrupted(_))));
    sim::fail(Fail::None);
    assert!(!ecu.arm(), "armed again already");
    let frames = ecu.exchange(3);
    assert_eq!(frames[2].0, 0x8001);
    assert_eq!(frames[2].1[4..], [0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    ecu.tick();
    assert_eq!(
        calls(),
        [Call::Session("oem", 0x03), Call::Session("platform", 0x03)]
    );
    assert_eq!(ecu.request(&[0x22, 0xF1, 0x86]), [0x62, 0xF1, 0x86, 0x03]);
}

/// A failed accept while a session change is prepared stops the server for good: the
/// change is abandoned, the server is left unarmed and is never armed again.
#[test]
fn a_failed_accept_mid_request_stops_the_server_for_good() {
    let _lock = lock();
    let mut ecu = Ecu::start();
    ecu.activate();
    *support::PREPARE.lock().unwrap() = Prepare::Pend;
    sim::send(&support::diagnostic(&[0x10, 0x03]));
    ecu.exchange(2);
    sim::fail(Fail::Accept);
    assert!(matches!(ecu.poll(), Err(RunError::Stopped(_))));
    sim::fail(Fail::None);
    assert!(!ecu.arm(), "armed again after a terminal error");
    assert!(matches!(ecu.poll(), Err(RunError::NotStarted)));
    assert_eq!(calls().last(), Some(&Call::Abandon("platform", 0x01, 0x03)));
}

/// A server never armed is not polled.
#[test]
fn an_unarmed_server_is_not_started() {
    let _lock = lock();
    assert!(matches!(poll_unarmed(), Err(RunError::NotStarted)));
}

/// The bench layers pass validation.
#[test]
fn the_bench_layers_validate() {
    assert!(App::validate().is_ok());
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
            get: None,
            snapshot_count: None,
            snapshot_item: None,
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
            abandon: None,
        },
        reset: uds_c::UdsTransitionHooks {
            check: None,
            prepare: None,
            execute: None,
            abandon: None,
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

unsafe extern "C" fn get_nothing(_index: usize, _dtc: *mut u32, _status: *mut u8) -> bool {
    false
}

unsafe extern "C" fn count_nothing(_index: usize) -> usize {
    0
}

unsafe extern "C" fn clear_nothing(_op: u8, _index: usize) -> u8 {
    0x00
}

/// A layer whose only hooks are the DTC ones `dtc` gives.
const fn layer_of_dtc(dtc: uds_c::UdsDtcHooks) -> UdsLayer {
    UdsLayer {
        dtc,
        ..layer_of(&[])
    }
}

const NO_DTC_HOOKS: uds_c::UdsDtcHooks = uds_c::UdsDtcHooks {
    get: None,
    snapshot_count: None,
    snapshot_item: None,
    clear: None,
    setting: None,
};

/// A DTC clear, or a snapshot, in a layer that keeps no DTC memory would clear or read
/// indices of another layer's memory, and a snapshot count without its items could not
/// be given: each is refused at start-up.
#[test]
fn dtc_hooks_without_their_memory_are_refused() {
    static CLEAR_ONLY: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        clear: Some(clear_nothing),
        ..NO_DTC_HOOKS
    });
    static MEMORY: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        get: Some(get_nothing),
        ..NO_DTC_HOOKS
    });
    static COUNT_ONLY: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        get: Some(get_nothing),
        snapshot_count: Some(count_nothing),
        ..NO_DTC_HOOKS
    });
    validated_only!(Split, CLEAR_ONLY, MEMORY);
    validated_only!(Counted, COUNT_ONLY);
    assert_eq!(
        TableApp::<Split>::validate(),
        Err(TableError::NoHandler(AccessOf::DtcMemory(0)))
    );
    assert_eq!(
        TableApp::<Counted>::validate(),
        Err(TableError::NoHandler(AccessOf::DtcMemory(0)))
    );
}

/// A second layer's DTC memory, which the server would never read, is refused at
/// start-up.
#[test]
fn a_second_dtc_memory_is_refused() {
    static FIRST: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        get: Some(get_nothing),
        ..NO_DTC_HOOKS
    });
    static SECOND: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        get: Some(get_nothing),
        clear: Some(clear_nothing),
        ..NO_DTC_HOOKS
    });
    validated_only!(TwoMemories, FIRST, SECOND);
    assert_eq!(
        TableApp::<TwoMemories>::validate(),
        Err(TableError::Duplicate(AccessOf::DtcMemory(1)))
    );
}

unsafe extern "C" fn set_nothing(_op: u8, _setting_type: u8) -> u8 {
    0x00
}

unsafe extern "C" fn control_nothing(_op: u8, _control: u8, _communication: u8) -> u8 {
    0x00
}

unsafe extern "C" fn begin_nothing(_op: u8, _address: u32, _size: u32, _format: u8) -> u8 {
    0x00
}

unsafe extern "C" fn block_nothing(_op: u8, _data: *const u8, _len: usize) -> u8 {
    0x00
}

unsafe extern "C" fn finish_nothing(_op: u8) -> u8 {
    0x00
}

/// A second layer's DTC setting, communication control or download, which the server
/// would never call, is refused at start-up.
#[test]
fn a_service_a_second_layer_serves_is_refused() {
    static SETTING: UdsLayer = layer_of_dtc(uds_c::UdsDtcHooks {
        setting: Some(set_nothing),
        ..NO_DTC_HOOKS
    });
    static CONTROL: UdsLayer = UdsLayer {
        comm_control: Some(control_nothing),
        ..layer_of(&[])
    };
    static DOWNLOAD: UdsLayer = UdsLayer {
        transfer: uds_c::UdsTransferHooks {
            access: UdsAccess {
                sessions: 0x02,
                security_level: 0,
            },
            begin: Some(begin_nothing),
            block: Some(block_nothing),
            finish: Some(finish_nothing),
        },
        ..layer_of(&[])
    };
    validated_only!(TwoSettings, SETTING, CONTROL, SETTING);
    validated_only!(TwoControls, CONTROL, CONTROL);
    validated_only!(TwoDownloads, DOWNLOAD, SETTING, DOWNLOAD);
    assert_eq!(
        TableApp::<TwoSettings>::validate(),
        Err(TableError::Duplicate(AccessOf::DtcSetting(2)))
    );
    assert_eq!(
        TableApp::<TwoControls>::validate(),
        Err(TableError::Duplicate(AccessOf::CommControl(1)))
    );
    assert_eq!(
        TableApp::<TwoDownloads>::validate(),
        Err(TableError::Duplicate(AccessOf::Transfer(2)))
    );
}

unsafe extern "C" fn seed_nothing(_op: u8, _option: *const u8, _option_len: usize) -> u8 {
    0x00
}

unsafe extern "C" fn key_nothing(_op: u8, _key: *const u8, _key_len: usize) -> u8 {
    0x00
}

/// A security level `level`, its seed `seed_len` bytes, locking out after `attempt_limit`
/// wrong keys.
const fn level(level: u8, seed_len: u8, attempt_limit: u8) -> UdsSecurityLevel {
    UdsSecurityLevel {
        level,
        sessions: 0x04,
        attempt_limit,
        seed_len,
        delay_ms: 1_000,
        seed: Some(seed_nothing),
        key: Some(key_nothing),
    }
}

/// A layer serving `levels` alone.
const fn layer_of_levels(levels: &'static [UdsSecurityLevel]) -> UdsLayer {
    UdsLayer {
        security_levels: levels.as_ptr(),
        security_level_count: levels.len(),
        ..layer_of(&[])
    }
}

/// A security level that is a sendKey value, not a requestSeed one, is refused at
/// start-up.
#[test]
fn a_level_that_is_not_a_request_seed_is_refused() {
    static LEVELS: [UdsSecurityLevel; 1] = [level(0x02, 2, 3)];
    static LAYER: UdsLayer = layer_of_levels(&LEVELS);
    validated_only!(SendKey, LAYER);
    assert_eq!(
        TableApp::<SendKey>::validate(),
        Err(TableError::NotRequestSeed { level: 0x02 })
    );
}

/// A seed that is empty, or longer than the policy allows, is refused at start-up.
#[test]
fn a_seed_of_no_or_too_many_bytes_is_refused() {
    static EMPTY_LEVELS: [UdsSecurityLevel; 1] = [level(0x01, 0, 3)];
    static LONG_LEVELS: [UdsSecurityLevel; 1] = [level(0x03, 33, 3)];
    static EMPTY: UdsLayer = layer_of_levels(&EMPTY_LEVELS);
    static LONG: UdsLayer = layer_of_levels(&LONG_LEVELS);
    validated_only!(EmptySeed, EMPTY);
    validated_only!(LongSeed, LONG);
    assert_eq!(
        TableApp::<EmptySeed>::validate(),
        Err(TableError::SeedLength { level: 0x01 })
    );
    assert_eq!(
        TableApp::<LongSeed>::validate(),
        Err(TableError::SeedLength { level: 0x03 })
    );
}

/// More security levels than the application counts attempts for are refused at
/// start-up.
#[test]
fn more_levels_than_the_application_tracks_are_refused() {
    static LEVELS: [UdsSecurityLevel; 5] = [
        level(0x01, 2, 3),
        level(0x03, 2, 3),
        level(0x05, 2, 3),
        level(0x07, 2, 3),
        level(0x09, 2, 3),
    ];
    static LAYER: UdsLayer = layer_of_levels(&LEVELS);
    validated_only!(Five, LAYER);
    assert_eq!(
        TableApp::<Five>::validate(),
        Err(TableError::TooManyLevels { count: 5 })
    );
}

/// A security level that would lock out before any key is tried is refused at start-up.
#[test]
fn a_level_allowing_no_attempt_is_refused() {
    static LEVELS: [UdsSecurityLevel; 1] = [level(0x01, 2, 0)];
    static LAYER: UdsLayer = layer_of_levels(&LEVELS);
    validated_only!(NoAttempt, LAYER);
    assert_eq!(
        TableApp::<NoAttempt>::validate(),
        Err(TableError::NoAttempts { level: 0x01 })
    );
}

/// An access naming a level that is not a requestSeed one is refused at start-up.
#[test]
fn an_access_naming_a_send_key_level_is_refused() {
    static DIDS: [UdsDidEntry; 1] = [UdsDidEntry {
        write: Some(write_nothing),
        write_access: UdsAccess {
            sessions: 0x04,
            security_level: 0x02,
        },
        ..readable(0x0300, 4)
    }];
    static LAYER: UdsLayer = layer_of(&DIDS);
    validated_only!(Typo, LAYER);
    assert_eq!(
        TableApp::<Typo>::validate(),
        Err(TableError::Access(AccessOf::Did(0x0300)))
    );
}

unsafe extern "C" fn write_nothing(_op: u8, _data: *const u8, _len: usize) -> u8 {
    0x00
}
