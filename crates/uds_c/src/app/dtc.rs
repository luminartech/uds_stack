//! The DTC services: `ReadDTCInformation` (0x19) and `ClearDiagnosticInformation` (0x14)
//! over the first layer that keeps a DTC memory, and `ControlDTCSetting` (0x85) over the
//! first layer that serves it.

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, CLEAR_ALL_DTCS, ClearDiagnosticInformation, ControlDtcSetting,
    DtcFormatIdentifier, DtcRecord, DtcReportKind, DtcSettingType, DtcSnapshotRecordNumber,
    DtcStatusMask, Encode, ReadDtcInfoReportType, ReadDtcInfoSubFunction,
    ReadDtcInformation, ResponseSink, Sink,
};

use super::{Policy, TableApp, everywhere, outside_default, settled};
use crate::layer::DtcMemory;
use crate::op;

/// reportNumberOfDTCByStatusMask, reportDTCByStatusMask,
/// reportDTCSnapshotIdentification, reportDTCSnapshotRecordByDTCNumber and
/// reportSupportedDTC.
const REPORT_TYPES: [u8; 5] = [0x01, 0x02, 0x03, 0x04, 0x0A];

/// `DTCFormatIdentifier`: the ISO 14229-1 DTC format.
const FORMAT: DtcFormatIdentifier = DtcFormatIdentifier::Iso14229_1DtcFormat;

/// The one snapshot record a DTC holds.
const SNAPSHOT_RECORD: DtcSnapshotRecordNumber = DtcSnapshotRecordNumber::new(0x01);

/// `DTCSnapshotRecordNumber` 0xFF: every record the DTC holds.
const ALL_SNAPSHOT_RECORDS: u8 = 0xFF;

/// `DTCSnapshotRecordNumberOfIdentifiers` 0x00, which ISO 14229-1 gives a record of more
/// identifiers than the byte counts.
const UNDEFINED_IDENTIFIERS: u8 = 0x00;

/// `UDS_DTC_ALL`: a clear of every DTC.
pub const UDS_DTC_ALL: usize = usize::MAX;

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ReadDtcInformation
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DTCS: usize = P::MAX_DTCS;
    const REPORTS: &'static [DtcReportKind] =
        &[DtcReportKind::Count, DtcReportKind::DtcList];

    fn access(&self, report: ReadDtcInfoReportType) -> Option<Access> {
        REPORT_TYPES.contains(&u8::from(report)).then(everywhere)
    }

    /// Encodes the report from the memory's DTCs, clause 12.3: a response longer than
    /// the buffer is `responseTooLong` (0x14), which the sink settles. The memory answers
    /// at once, so the report is ready when asked.
    fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), Nrc>> {
        let memory = P::LAYERS.dtc_memory().map(|memory| Memory {
            memory,
            availability: P::DTC_AVAILABILITY,
        });
        core::future::ready(report(memory, request, out))
    }
}

/// The DTC memory the reports read, and the `DTCStatusAvailabilityMask` they carry: the
/// status bits it supports, the only ones a report matches or returns.
#[derive(Debug, Clone, Copy)]
struct Memory {
    memory: DtcMemory,
    availability: DtcStatusMask,
}

impl Memory {
    /// Every DTC, in index order: its index, number and status within the availability
    /// mask.
    fn dtcs(self) -> impl Iterator<Item = (usize, u32, DtcStatusMask)> {
        self.memory.dtcs().map(move |(index, dtc, status)| {
            (index, dtc, DtcStatusMask::from(status) & self.availability)
        })
    }
}

/// The report `request` asks for, from `memory`.
fn report(
    memory: Option<Memory>,
    request: ReadDtcInfoSubFunction,
    out: &mut ResponseSink<'_>,
) -> Result<(), Nrc> {
    let Some(memory) = memory else {
        return Err(Nrc::ServiceNotSupported);
    };
    match request {
        ReadDtcInfoSubFunction::ReportNumberOfDtcByStatusMask(mask) => {
            let count = memory
                .dtcs()
                .filter(|&(_, _, status)| matches(status, mask))
                .count();
            let _ = memory.availability.encode(out);
            let _ = out.write_all(&[u8::from(FORMAT)]);
            let _ = out.write_all(&u16::try_from(count).unwrap_or(u16::MAX).to_be_bytes());
        }
        ReadDtcInfoSubFunction::ReportDtcByStatusMask(mask) => {
            list(memory, out, Some(mask));
        }
        ReadDtcInfoSubFunction::ReportSupportedDtc => list(memory, out, None),
        ReadDtcInfoSubFunction::ReportDtcSnapshotIdentification => {
            for (index, dtc, _) in memory.dtcs() {
                if memory.memory.snapshot_count(index) > 0 {
                    let _ = record(dtc).encode(out);
                    let _ = SNAPSHOT_RECORD.encode(out);
                }
            }
        }
        ReadDtcInfoSubFunction::ReportDtcSnapshotRecordByDtcNumber(dtc, number) => {
            snapshot(memory, out, dtc, u8::from(number))?;
        }
        _ => return Err(Nrc::SubFunctionNotSupported),
    }
    Ok(())
}

/// The availability mask, then each DTC whose status matches `mask` with its status,
/// or every DTC where there is no mask (reportSupportedDTC).
fn list(memory: Memory, out: &mut ResponseSink<'_>, mask: Option<DtcStatusMask>) {
    let _ = memory.availability.encode(out);
    for (_, dtc, status) in memory.dtcs() {
        if mask.is_none_or(|mask| matches(status, mask)) {
            let _ = record(dtc).encode(out);
            let _ = status.encode(out);
        }
    }
}

/// Whether `status` matches `mask`: some bit is set in both. An empty mask matches
/// nothing, where `DtcStatusMask::intersects` would match everything.
fn matches(status: DtcStatusMask, mask: DtcStatusMask) -> bool {
    (status & mask).bits() != 0
}

/// The DTC and its status, then its snapshot record where it holds one: the record
/// number, the number of identifiers, then each identifier and its data. A DTC the
/// memory does not hold, or a record number other than the one record or all of them,
/// is `requestOutOfRange` (0x31).
fn snapshot(
    memory: Memory,
    out: &mut ResponseSink<'_>,
    dtc: DtcRecord,
    number: u8,
) -> Result<(), Nrc> {
    let wanted = dtc.to_u32();
    let Some((index, _, status)) = memory.dtcs().find(|&(_, held, _)| held == wanted)
    else {
        return Err(Nrc::RequestOutOfRange);
    };
    if number != SNAPSHOT_RECORD.value() && number != ALL_SNAPSHOT_RECORDS {
        return Err(Nrc::RequestOutOfRange);
    }
    let _ = dtc.encode(out);
    let _ = status.encode(out);
    let memory = memory.memory;
    let items = memory.snapshot_count(index);
    if items > 0 {
        let _ = SNAPSHOT_RECORD.encode(out);
        let _ = out.write_all(&[u8::try_from(items).unwrap_or(UNDEFINED_IDENTIFIERS)]);
        for item in 0..items {
            memory
                .snapshot_item(index, item, |did, data| {
                    let _ = out.write_all(&did.to_be_bytes());
                    let _ = out.write_all(data);
                })
                .ok_or(Nrc::GeneralReject)?;
        }
    }
    Ok(())
}

/// A DTC number the memory holds, as the three bytes of a `DTCRecord`.
fn record(dtc: u32) -> DtcRecord {
    let [_, high, middle, low] = dtc.to_be_bytes();
    DtcRecord::new(high, middle, low)
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ClearDiagnosticInformation
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;

    /// Clears every DTC for the all-groups `groupOfDTC`, or the one DTC it names; any
    /// other group, and a `memorySelection` (the handler clears from the one memory it
    /// has), is `requestOutOfRange` (0x31), ISO 14229-1:2020 Figure 28.
    async fn clear(&mut self, group: DtcRecord, memory: Option<u8>) -> Result<(), Nrc> {
        let Some((dtcs, clear)) = P::LAYERS
            .dtc_memory()
            .and_then(|dtcs| Some((dtcs, dtcs.clear()?)))
        else {
            return Err(Nrc::ServiceNotSupported);
        };
        if memory.is_some() {
            return Err(Nrc::RequestOutOfRange);
        }
        let index = if group == CLEAR_ALL_DTCS {
            UDS_DTC_ALL
        } else {
            let wanted = group.to_u32();
            dtcs.dtcs()
                .find(|&(_, dtc, _)| dtc == wanted)
                .map(|(index, _, _)| index)
                .ok_or(Nrc::RequestOutOfRange)?
        };
        settled(op::settle(|op| clear.call(op, index)).await)
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ControlDtcSetting
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;
    /// The server has one DTC group, so a request naming one in an option record is
    /// `incorrectMessageLengthOrInvalidFormat`.
    const MAX_OPTION_RECORD_LEN: usize = 0;

    /// `on` (0x01) and `off` (0x02), in the extended and programming sessions.
    fn access(&self, setting: DtcSettingType) -> Option<Access> {
        matches!(u8::from(setting), 0x01 | 0x02).then(outside_default)
    }

    async fn control_dtc_setting(
        &mut self,
        setting: DtcSettingType,
        _option_record: &[u8],
    ) -> Result<(), Nrc> {
        let Some(control) = P::LAYERS.dtc_setting() else {
            return Err(Nrc::ServiceNotSupported);
        };
        let setting = u8::from(setting);
        settled(op::settle(|op| control.call(op, setting)).await)
    }
}
