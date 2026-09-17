//! Stored data transmission — ISO 14229-1:2020 clause 12.

use crate::ResponseSink;
use uds_protocol::NegativeResponseCode;

/// `ReadDTCInformation` (0x19).
///
/// The sub-function passes through raw: clause 12.3 defines more than twenty report
/// types, which of them a server implements is the application's, and the record formats
/// are `uds_protocol`'s.
pub trait ReadDtcInformation {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The most DTC records this server returns in one response.
    ///
    /// Folded into the response buffer. A server with more stored DTCs than this cannot
    /// report them all in one message, which is a deployment decision rather than a
    /// defect — clause 12.3 provides paged report types for that case.
    const MAX_DTCS: usize;

    /// Bytes per DTC record, including the status byte.
    const DTC_RECORD_LEN: usize;

    /// Write the report named by `report_type` into `out`.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unsupported report type or out-of-range mask.
    fn read_dtc_information(
        &mut self,
        report_type: u8,
        parameters: &[u8],
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ClearDiagnosticInformation` (0x14).
pub trait ClearDiagnosticInformation {
    /// ``UDSSVC_ARCH_0033`` — clearing stored data is frequently slow enough to need one.
    const MAY_RESPOND_PENDING: bool;

    /// Clear the DTCs selected by `group`, optionally restricted to one memory.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unknown group or a clear that failed.
    fn clear(
        &mut self,
        group: u32,
        memory_selection: Option<u8>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}
