//! Stored data transmission — ISO 14229-1:2020 clause 12.

use crate::ResponseSink;
use uds_protocol::{
    FunctionalGroupIdentifier, NegativeResponseCode, ReadDtcInfoSubFunction,
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
#[non_exhaustive]
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
    /// Bytes this layout writes before its records, including the `0x59` service
    /// identifier.
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
/// implements is the application's. Each arrives decoded as a
/// [`ReadDtcInfoSubFunction`], which carries that report type's own parameters — a
/// status mask, a record number, a [`DtcRecord`](uds_protocol::DtcRecord) — so a
/// handler matches on the report it was asked for rather than parsing the bytes behind
/// it.
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

    /// Write the report `request` asks for into `out`.
    ///
    /// # Arguments
    ///
    /// * `request` - the report type and its parameters; see
    ///   [`ReadDtcInfoSubFunction`]. There is no separate parameter slice: a report
    ///   type's parameters are fixed-width and ride on its variant, so a malformed one
    ///   is rejected with `incorrectMessageLengthOrInvalidFormat` (0x13) before it
    ///   reaches here.
    /// * `out` - where the report's **records** are written. The header the layout
    ///   carries — the service identifier, the sub-function echo and the status
    ///   availability mask — is written by the pipeline, as
    ///   [`ReadDataByIdentifier::read`](crate::ReadDataByIdentifier::read)'s identifier
    ///   is. That split is what makes [`DtcReportKind::header_len`] this crate's number
    ///   to know.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a report type this server does not implement.
    fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `ClearDiagnosticInformation` (0x14).
pub trait ClearDiagnosticInformation {
    /// ``UDSSVC_ARCH_0033`` — clearing stored data is frequently slow enough to need one.
    const MAY_RESPOND_PENDING: bool;

    /// Clear the DTCs `group` selects, optionally restricted to one memory.
    ///
    /// # Arguments
    ///
    /// * `group` - which DTCs to clear; see [`FunctionalGroupIdentifier`] and
    ///   [`CLEAR_ALL_DTCS`](uds_protocol::CLEAR_ALL_DTCS).
    /// * `memory_selection` - the `MemorySelection` byte, where the request carried one.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an unknown group or a clear that failed.
    fn clear(
        &mut self,
        group: FunctionalGroupIdentifier,
        memory_selection: Option<u8>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}
