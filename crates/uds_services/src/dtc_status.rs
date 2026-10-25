//! The DTC status transitions of ISO 14229-1:2020 Annex D.2, ``UDSSVC_ARCH_0047``.
//!
//! A server's DTC memory is the application's: which DTCs it holds, when a test result
//! matures, what an operation cycle is, and where the statuses are stored. How a status
//! byte changes on a matured result, at an operation cycle's start and on a clear is the
//! standard's, and these functions make those changes, so a memory applies the annex
//! rather than restating it. Each is a pure `const fn` of a status, which a memory may
//! call from any task.
//!
//! They maintain every bit but [`WarningIndicatorRequested`], whose condition is the
//! application's and which they leave as they find it, and confirm a DTC at its first
//! failure: a memory with a confirmation threshold sets `confirmedDTC` itself.
//!
//! [`WarningIndicatorRequested`]: DtcStatusMask::WarningIndicatorRequested

use crate::DtcStatusMask;

/// The bits these transitions maintain, every bit but
/// [`WarningIndicatorRequested`](DtcStatusMask::WarningIndicatorRequested): the
/// `DTCStatusAvailabilityMask` of a server whose memory makes only these.
pub const MAINTAINED: DtcStatusMask = DtcStatusMask::WarningIndicatorRequested.not();

/// A DTC's status after `ClearDiagnosticInformation`, and a new DTC's: not tested since
/// the clear nor in this operation cycle.
#[must_use]
pub const fn cleared() -> DtcStatusMask {
    DtcStatusMask::TestNotCompletedSinceLastClear
        .or(DtcStatusMask::TestNotCompletedThisOperationCycle)
}

/// `status` after a matured test result, `failed` or passed: the test has completed since
/// the last clear and in this operation cycle; a failure sets `testFailed`,
/// `testFailedThisOperationCycle`, `pendingDTC`, `confirmedDTC` and
/// `testFailedSinceLastClear`, and a pass clears `testFailed`.
#[must_use]
pub const fn after_test_result(status: DtcStatusMask, failed: bool) -> DtcStatusMask {
    let completed = status.and(cleared().not());
    if failed {
        completed
            .or(DtcStatusMask::TestFailed)
            .or(DtcStatusMask::TestFailedThisOperationCycle)
            .or(DtcStatusMask::PendingDtc)
            .or(DtcStatusMask::ConfirmedDtc)
            .or(DtcStatusMask::TestFailedSinceLastClear)
    } else {
        completed.and(DtcStatusMask::TestFailed.not())
    }
}

/// The status an operation cycle starts with, from the `status` the last one ended with:
/// that cycle ends, so `pendingDTC` clears where it tested the DTC without a failure, and
/// this one has neither tested nor failed it yet. `testFailed` clears with it, until the
/// test reports again in the new cycle.
#[must_use]
pub const fn at_operation_cycle_start(status: DtcStatusMask) -> DtcStatusMask {
    let kept = status.and(
        DtcStatusMask::PendingDtc
            .or(DtcStatusMask::ConfirmedDtc)
            .or(DtcStatusMask::TestNotCompletedSinceLastClear)
            .or(DtcStatusMask::TestFailedSinceLastClear)
            .or(DtcStatusMask::WarningIndicatorRequested),
    );
    let tested_without_failure = !status.intersects(
        DtcStatusMask::TestFailedThisOperationCycle
            .or(DtcStatusMask::TestNotCompletedThisOperationCycle),
    );
    let kept = if tested_without_failure {
        kept.and(DtcStatusMask::PendingDtc.not())
    } else {
        kept
    };
    kept.or(DtcStatusMask::TestNotCompletedThisOperationCycle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleared_dtc_has_completed_no_test() {
        assert_eq!(cleared().bits(), 0x50);
        assert_eq!(MAINTAINED.bits(), 0x7F);
    }

    #[test]
    fn a_failure_confirms_and_a_pass_clears_only_test_failed() {
        let failed = after_test_result(cleared(), true);
        assert_eq!(failed.bits(), 0x2F);
        assert_eq!(after_test_result(failed, false).bits(), 0x2E);
        assert_eq!(after_test_result(cleared(), false).bits(), 0x00);
    }

    #[test]
    fn a_cycle_that_tested_without_failure_clears_pending() {
        // Failed in an earlier cycle, passed in the last: pending clears, confirmed stays.
        let passed = DtcStatusMask::PendingDtc
            | DtcStatusMask::ConfirmedDtc
            | DtcStatusMask::TestFailedSinceLastClear;
        assert_eq!(at_operation_cycle_start(passed).bits(), 0x68);
        // Failed in the last cycle: pending stays, the cycle's own bits restart.
        let failed = after_test_result(cleared(), true);
        assert_eq!(at_operation_cycle_start(failed).bits(), 0x6C);
        // Not tested in the last cycle: pending stays.
        let untested = passed | DtcStatusMask::TestNotCompletedThisOperationCycle;
        assert_eq!(at_operation_cycle_start(untested).bits(), 0x6C);
        assert_eq!(at_operation_cycle_start(cleared()), cleared());
    }

    #[test]
    fn the_warning_indicator_is_left_to_the_application() {
        let lit = DtcStatusMask::WarningIndicatorRequested | cleared();
        let warning = DtcStatusMask::WarningIndicatorRequested;
        assert!(after_test_result(lit, false).contains(warning));
        assert!(at_operation_cycle_start(lit).contains(warning));
    }
}
