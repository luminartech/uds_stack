//! The DTC services: `ReadDTCInformation` (0x19) and `ClearDiagnosticInformation` (0x14)
//! over the first layer that keeps a DTC memory, and `ControlDTCSetting` (0x85) over the
//! first layer that serves it.

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, CLEAR_ALL_DTCS, ClearDiagnosticInformation, ControlDtcSetting,
    DtcFormatIdentifier, DtcRecord, DtcReportKind, DtcSettingType, DtcSnapshotRecordNumber,
    DtcStatusMask, Encode, ReadDtcInfoReportType, ReadDtcInfoSubFunction,
    ReadDtcInformation, ResponseSink, Sink, dtc_status,
};

use super::{Policy, TableApp, everywhere, outside_default, settled};
use crate::layer::DtcMemory;
use crate::op;

/// reportNumberOfDTCByStatusMask, reportDTCByStatusMask,
/// reportDTCSnapshotIdentification, reportDTCSnapshotRecordByDTCNumber and
/// reportSupportedDTC.
const REPORT_TYPES: [u8; 5] = [0x01, 0x02, 0x03, 0x04, 0x0A];

/// The `DTCStatusAvailabilityMask`: the bits maintained by the status transitions the
/// memory makes ([`dtc_status::MAINTAINED`]).
const AVAILABILITY: DtcStatusMask = dtc_status::MAINTAINED;

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

    /// Where a layer keeps a DTC memory.
    fn serves(&self) -> bool {
        P::LAYERS.dtc_memory().is_some()
    }

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
        core::future::ready(report(P::LAYERS.dtc_memory(), request, out))
    }
}

/// The report `request` asks for, from `memory`.
fn report(
    memory: Option<DtcMemory>,
    request: ReadDtcInfoSubFunction,
    out: &mut ResponseSink<'_>,
) -> Result<(), Nrc> {
    let Some(memory) = memory else {
        return Err(Nrc::ServiceNotSupported);
    };
    match request {
        ReadDtcInfoSubFunction::ReportNumberOfDtcByStatusMask(mask) => {
            let mask = mask & AVAILABILITY;
            let count = memory
                .dtcs()
                .filter(|&(_, _, status)| matches(DtcStatusMask::from(status), mask))
                .count();
            let _ = AVAILABILITY.encode(out);
            let _ = out.write_all(&[u8::from(FORMAT)]);
            let _ = out.write_all(&u16::try_from(count).unwrap_or(u16::MAX).to_be_bytes());
        }
        ReadDtcInfoSubFunction::ReportDtcByStatusMask(mask) => {
            list(memory, out, Some(mask & AVAILABILITY));
        }
        ReadDtcInfoSubFunction::ReportSupportedDtc => list(memory, out, None),
        ReadDtcInfoSubFunction::ReportDtcSnapshotIdentification => {
            for (index, dtc, _) in memory.dtcs() {
                if memory.snapshot_count(index) > 0 {
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
fn list(memory: DtcMemory, out: &mut ResponseSink<'_>, mask: Option<DtcStatusMask>) {
    let _ = AVAILABILITY.encode(out);
    for (_, dtc, status) in memory.dtcs() {
        let status = DtcStatusMask::from(status);
        if mask.is_none_or(|mask| matches(status, mask)) {
            let _ = record(dtc).encode(out);
            let _ = (status & AVAILABILITY).encode(out);
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
    memory: DtcMemory,
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
    let _ = (DtcStatusMask::from(status) & AVAILABILITY).encode(out);
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

    /// Where the layer that keeps the DTC memory clears it.
    fn serves(&self) -> bool {
        P::LAYERS.dtc_memory().and_then(DtcMemory::clear).is_some()
    }

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
    const MAX_OPTION_RECORD_LEN: usize = 3;

    /// Where a layer serves it.
    fn serves(&self) -> bool {
        P::LAYERS.dtc_setting().is_some()
    }

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
