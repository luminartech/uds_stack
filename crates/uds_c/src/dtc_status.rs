//! The DTC status transitions of ISO 14229-1:2020 Annex D.2, as
//! [`uds_services::dtc_status`] gives them, for a DTC memory the firmware keeps. Each is
//! a pure function of a status byte, so unlike the server's other calls any task may make
//! it.

#![allow(
    unsafe_code,
    reason = "exports the transitions to C under fixed symbol names"
)]

use uds_services::{DtcStatusMask, dtc_status};

/// `uds_dtc_status_cleared`: a DTC's status after `ClearDiagnosticInformation`, and a
/// new DTC's.
#[unsafe(no_mangle)]
pub extern "C" fn uds_dtc_status_cleared() -> u8 {
    dtc_status::cleared().bits()
}

/// `uds_dtc_status_after_test_result`: `status` after a matured test result, `failed`
/// or passed.
#[unsafe(no_mangle)]
pub extern "C" fn uds_dtc_status_after_test_result(status: u8, failed: bool) -> u8 {
    dtc_status::after_test_result(DtcStatusMask::from(status), failed).bits()
}

/// `uds_dtc_status_at_operation_cycle_start`: the status a new operation cycle starts
/// with, from the `status` the last one ended with.
#[unsafe(no_mangle)]
pub extern "C" fn uds_dtc_status_at_operation_cycle_start(status: u8) -> u8 {
    dtc_status::at_operation_cycle_start(DtcStatusMask::from(status)).bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c_transitions_are_the_protocols() {
        let cleared = uds_dtc_status_cleared();
        assert_eq!(cleared, 0x50);
        let failed = uds_dtc_status_after_test_result(cleared, true);
        assert_eq!(failed, 0x2F);
        assert_eq!(uds_dtc_status_at_operation_cycle_start(failed), 0x6C);
    }
}
