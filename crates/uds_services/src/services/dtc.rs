//! Stored data transmission — ISO 14229-1:2020 clause 12.

use crate::{Access, ResponseSink};
use uds_protocol::{
    DtcRecord, NegativeResponseCode, ReadDtcInfoReportType, ReadDtcInfoSubFunction,
};

/// Which shape of `ReadDTCInformation` response a report type produces.
///
/// ISO 14229-1:2020 clause 12.3 gives each report type one of five response layouts, and
/// the layout fixes the header and record widths — they are not the application's to
/// choose. A server declares the layouts it answers in [`ReadDtcInformation::REPORTS`]
/// and [`crate::uds_server`] folds the widest of them into the response buffer.
///
/// The five correspond one-to-one to [`uds_protocol::ReadDtcInfoResponse`]'s variants,
/// which is where the widths below are read from rather than restated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DtcReportKind {
    /// A count and no records — sub-functions `0x01` and `0x07`.
    Count,
    /// `(DTC, status)` pairs — `0x02`, `0x0A`-`0x0E`, `0x15`.
    DtcList,
    /// `(DTC, fault detection counter)` pairs — `0x14`.
    FaultDetectionCounters,
    /// `DTCAndSeverityRecord`s, which carry a severity and a `DTCFunctionalUnit` byte
    /// that [`Self::DtcList`]'s records do not — `0x08` and `0x09`.
    SeverityList,
    /// WWH-OBD severity records — `0x42`.
    WwhObdSeverity,
}

impl DtcReportKind {
    /// Bytes this layout puts before its records, including the `0x59` service
    /// identifier and the sub-function echo the pipeline writes.
    #[must_use]
    pub const fn header_len(self) -> usize {
        match self {
            Self::Count | Self::WwhObdSeverity => 6,
            Self::DtcList | Self::SeverityList => 3,
            Self::FaultDetectionCounters => 2,
        }
    }

    /// Bytes per record, or zero for a layout that carries none.
    #[must_use]
    pub const fn record_len(self) -> usize {
        match self {
            Self::Count => 0,
            Self::DtcList | Self::FaultDetectionCounters => 4,
            Self::WwhObdSeverity => 5,
            Self::SeverityList => 6,
        }
    }
}

/// `ReadDTCInformation` (0x19).
///
/// Clause 12.3 defines more than twenty report types, and which of them a server
/// implements is the application's. Figure 6's questions are asked of the report type
/// alone, a [`ReadDtcInfoReportType`], before the request's parameters are decoded; the
/// report then arrives decoded as a [`ReadDtcInfoSubFunction`], which carries that report
/// type's own parameters — a status mask, a record number, a
/// [`DtcRecord`] — so a handler matches on the report it was asked for rather than
/// parsing the bytes behind it.
pub trait ReadDtcInformation {
    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The most DTC records this server returns in one response.
    ///
    /// Folded into the response buffer. A server with more stored DTCs than this cannot
    /// report them all in one message, which is a deployment decision rather than a
    /// defect — clause 12.3 provides paged report types for that case.
    const MAX_DTCS: usize;

    /// The response layouts this server answers; see [`DtcReportKind`].
    ///
    /// [`crate::uds_server`] folds `max(header + MAX_DTCS * record)` over these into the
    /// response buffer. Declaring the layouts rather than a width is what makes the
    /// buffer right: a server answering [`DtcReportKind::SeverityList`] needs six-byte
    /// records where [`DtcReportKind::DtcList`] needs four, and neither width is the
    /// application's to know.
    const REPORTS: &'static [DtcReportKind];

    /// Where `report` is available and which levels unlock it, or `None` where this
    /// server does not support it.
    ///
    /// ISO 14229-1:2020 8.7.3.1 Figure 6, ``UDSSVC_ARCH_0007`` — in that order, `None`
    /// settles the request `subFunctionNotSupported` (0x12), an active session outside
    /// [`Access::sessions`] `subFunctionNotSupportedInActiveSession` (0x7E), and none of
    /// [`Access::levels`] unlocked `securityAccessDenied` (0x33), each before the
    /// request's parameters are decoded and without [`Self::read_dtc_information`] being
    /// asked.
    ///
    /// # Arguments
    ///
    /// * `report` - the report type requested; see [`ReadDtcInfoReportType`], whose
    ///   reserved variant carries the raw byte.
    fn access(&self, report: ReadDtcInfoReportType) -> Option<Access>;

    /// Write the report `request` asks for into `out`.
    ///
    /// # Arguments
    ///
    /// * `request` - the report type and its parameters; see
    ///   [`ReadDtcInfoSubFunction`], whose [`report_type`] [`Self::access`] admitted.
    ///   There is no separate parameter slice: a report type's parameters are
    ///   fixed-width and ride on its variant, so a malformed one is rejected with
    ///   `incorrectMessageLengthOrInvalidFormat` (0x13) before it reaches here.
    /// * `out` - where the response is written after the `59` service identifier and
    ///   the echoed report type, which the pipeline writes: everything after `59 xx` is
    ///   the handler's, whatever the report type's layout puts first. Clause 12.3.3 gives
    ///   each report type's layout; [`DtcReportKind::header_len`] counts the pipeline's
    ///   two bytes with the rest of the header.
    ///
    /// [`report_type`]: ReadDtcInfoSubFunction::report_type
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] where the report cannot be given, clause 12.3.4:
    /// `requestOutOfRange` (0x31) for a `DTCMaskRecord` the server does not recognise or
    /// an invalid record number.
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ClearDiagnosticInformation` (0x14).
///
/// ISO 14229-1:2020 clause 12.2, Figure 28 — the pipeline settles a request that is not
/// four or five bytes (0x13); every other check is the handler's.
pub trait ClearDiagnosticInformation {
    /// ``UDSSVC_ARCH_0033`` — clearing stored data is frequently slow enough to need one.
    const MAY_RESPOND_PENDING: bool;

    /// Clear the DTCs `group` selects, optionally restricted to one memory.
    ///
    /// Clause 12.2.1: the positive response is owed even where no DTC is stored.
    ///
    /// # Arguments
    ///
    /// * `group` - the `groupOfDTC`: a group of DTCs or a single one (Annex D.1); see
    ///   [`DtcRecord`] and [`CLEAR_ALL_DTCS`](uds_protocol::CLEAR_ALL_DTCS).
    /// * `memory_selection` - the `MemorySelection` byte, where the request carried one.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`], in Figure 28's order: `requestOutOfRange` (0x31) for
    /// an unsupported `memory_selection` or `group`, `conditionsNotCorrect` (0x22) where
    /// the server cannot clear, and `generalProgrammingFailure` (0x72) where the clear
    /// failed.
    fn clear(
        &mut self,
        group: DtcRecord,
        memory_selection: Option<u8>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}
