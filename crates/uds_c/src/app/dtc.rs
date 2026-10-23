//! The DTC services over the first layer that serves each: `ReadDTCInformation` (0x19),
//! `ClearDiagnosticInformation` (0x14) and `ControlDTCSetting` (0x85).

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, ClearDiagnosticInformation, ControlDtcSetting, DtcRecord, DtcReportKind,
    DtcSettingType, ReadDtcInfoReportType, ReadDtcInfoSubFunction, ReadDtcInformation,
    ResponseSink, Sink,
};

use super::{Policy, TableApp, everywhere, outside_default, settled};
use crate::op;

/// reportNumberOfDTCByStatusMask, reportDTCByStatusMask,
/// reportDTCSnapshotIdentification, reportDTCSnapshotRecordByDTCNumber and
/// reportSupportedDTC.
const REPORT_TYPES: [u8; 5] = [0x01, 0x02, 0x03, 0x04, 0x0A];

/// The longest report a handler copies for `dtcs` DTCs: the count report's four bytes,
/// or the availability mask and four bytes per DTC.
pub(super) const fn report_len(dtcs: usize) -> usize {
    let list = dtcs.saturating_mul(4).saturating_add(1);
    if list > 4 { list } else { 4 }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ReadDtcInformation
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DTCS: usize = P::MAX_DTCS;
    const REPORTS: &'static [DtcReportKind] =
        &[DtcReportKind::Count, DtcReportKind::DtcList];

    /// Where a layer serves it.
    fn serves(&self) -> bool {
        P::LAYERS.dtc_report().is_some()
    }

    fn access(&self, report: ReadDtcInfoReportType) -> Option<Access> {
        REPORT_TYPES.contains(&u8::from(report)).then(everywhere)
    }

    /// Answers as many bytes as the handler copied.
    async fn read_dtc_information(
        &mut self,
        request: ReadDtcInfoSubFunction,
        out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        let Some(report) = P::LAYERS.dtc_report() else {
            return Err(Nrc::ServiceNotSupported);
        };
        let (mask, dtc, record) = match request {
            ReadDtcInfoSubFunction::ReportNumberOfDtcByStatusMask(mask)
            | ReadDtcInfoSubFunction::ReportDtcByStatusMask(mask) => (u8::from(mask), 0, 0),
            ReadDtcInfoSubFunction::ReportDtcSnapshotRecordByDtcNumber(dtc, record) => {
                (0, dtc.to_u32(), u8::from(record))
            }
            _ => (0, 0, 0),
        };
        let report_type = u8::from(request.report_type());
        let len = report_len(P::MAX_DTCS);
        let mut answer = self.answer(len, len)?;
        settled(
            op::settle(|op| {
                answer.lend(|| report.call(op, report_type, mask, dtc, record))
            })
            .await,
        )?;
        let _ = out.write_all(answer.filled());
        Ok(())
    }
}

impl<P: Policy, const RECORD: usize, const LEVELS: usize> ClearDiagnosticInformation
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;

    /// Where a layer serves it.
    fn serves(&self) -> bool {
        P::LAYERS.dtc_clear().is_some()
    }

    /// The handler clears from the one memory it has, so a `memorySelection` is
    /// `requestOutOfRange` (0x31), ISO 14229-1:2020 Figure 28.
    async fn clear(&mut self, group: DtcRecord, memory: Option<u8>) -> Result<(), Nrc> {
        let Some(clear) = P::LAYERS.dtc_clear() else {
            return Err(Nrc::ServiceNotSupported);
        };
        if memory.is_some() {
            return Err(Nrc::RequestOutOfRange);
        }
        settled(op::settle(|op| clear.call(op, group.to_u32())).await)
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

    /// `on` (0x01) and `off` (0x02), outside the default session.
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

#[cfg(test)]
mod tests {
    use super::report_len;

    #[test]
    fn a_report_holds_the_count_or_every_dtc() {
        assert_eq!(report_len(0), 4);
        assert_eq!(report_len(64), 257);
    }
}
