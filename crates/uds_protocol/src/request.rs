//! Module for making and handling UDS Requests
use crate::{
    Decode, Encode, Error, Incomplete, NegativeResponseCode,
    services::{
        ClearDiagnosticInfoRequest, CommunicationControlRequest, ControlDtcSettingRequest,
        DiagnosticSessionControlRequest, EcuResetRequest, ReadDataByIdentifierRequest,
        ReadDtcInfoRequest, RequestDownloadRequest, RequestFileTransferRequest,
        RequestTransferExitRequest, RequestUploadRequest, RoutineControlRequest,
        SecurityAccessRequest, TesterPresentRequest, TransferDataRequest,
        WriteDataByIdentifierRequest,
    },
    shared::split_sprmib,
};
use automotive_wire_codec::{write_bytes, write_u8};

use super::service::UdsServiceType;

/// Zero-copy parsed request. Borrows from the wire buffer.
///
/// Variable-length payloads are stored as raw `&'a [u8]` slices that can be
/// further parsed on demand.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Request<'a> {
    /// Clear diagnostic information request.
    ClearDiagnosticInfo(ClearDiagnosticInfoRequest),
    /// Communication control request.
    CommunicationControl(CommunicationControlRequest),
    /// Control DTC settings request.
    ControlDtcSetting(ControlDtcSettingRequest<'a>),
    /// Diagnostic session control request.
    DiagnosticSessionControl(DiagnosticSessionControlRequest),
    /// ECU reset request.
    EcuReset(EcuResetRequest),
    /// Read data by identifier request.
    ReadDataByIdentifier(ReadDataByIdentifierRequest<'a>),
    /// Read DTC information request.
    ReadDtcInfo(ReadDtcInfoRequest),
    /// Request download.
    RequestDownload(RequestDownloadRequest),
    /// Request file transfer.
    RequestFileTransfer(RequestFileTransferRequest<'a>),
    /// Request transfer exit.
    RequestTransferExit(RequestTransferExitRequest<'a>),
    /// Request upload.
    RequestUpload(RequestUploadRequest),
    /// Routine control request.
    RoutineControl(RoutineControlRequest<'a>),
    /// Security access request.
    SecurityAccess(SecurityAccessRequest<'a>),
    /// Tester present request.
    TesterPresent(TesterPresentRequest),
    /// Transfer data request.
    TransferData(TransferDataRequest<'a>),
    /// Write data by identifier request.
    WriteDataByIdentifier(WriteDataByIdentifierRequest<'a>),
    /// A known-but-unmodeled (or unrecognized) service. Carries the raw service byte and
    /// the raw payload bytes following the service identifier, for pass-through.
    ///
    /// Re-encoding is lossless for every service byte: the raw `sid` is echoed verbatim.
    Other {
        /// The raw service identifier byte from the wire.
        sid: u8,
        /// Raw payload bytes after the service byte.
        data: &'a [u8],
    },
}

/// # Remainder
///
/// The returned remainder is **always empty**. A UDS frame is not self-delimiting: its
/// length comes from the transport (ISO-TP, `DoIP`, ...), not from the message, so one
/// buffer is exactly one frame, and every payload is decoded with `decode_exact`. This
/// means `decode` behaves as `decode_exact` despite the streaming shape of the [`Decode`]
/// contract: do **not** feed it concatenated frames expecting it to consume one at a time,
/// and note that [`DecodeIter`](crate::DecodeIter) over such a buffer would treat the whole
/// thing as a single frame. Split frames at the transport layer before calling this.
impl<'a> Decode<'a> for Request<'a> {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        let service = UdsServiceType::from_request_sid(buf[0]);
        let payload = &buf[1..];

        let request = match service {
            UdsServiceType::ClearDiagnosticInfo => Self::ClearDiagnosticInfo(
                <ClearDiagnosticInfoRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::CommunicationControl => Self::CommunicationControl(
                <CommunicationControlRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::ControlDtcSetting => Self::ControlDtcSetting(
                <ControlDtcSettingRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::DiagnosticSessionControl => Self::DiagnosticSessionControl(
                <DiagnosticSessionControlRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::EcuReset => {
                Self::EcuReset(<EcuResetRequest as Decode>::decode_exact(payload)?)
            }
            UdsServiceType::ReadDataByIdentifier => Self::ReadDataByIdentifier(
                <ReadDataByIdentifierRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::ReadDtcInfo => {
                Self::ReadDtcInfo(<ReadDtcInfoRequest as Decode>::decode_exact(payload)?)
            }
            UdsServiceType::RequestDownload => Self::RequestDownload(
                <RequestDownloadRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::RequestFileTransfer => Self::RequestFileTransfer(
                <RequestFileTransferRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::RequestTransferExit => Self::RequestTransferExit(
                <RequestTransferExitRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::RequestUpload => Self::RequestUpload(
                <RequestUploadRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::RoutineControl => Self::RoutineControl(
                <RoutineControlRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::SecurityAccess => Self::SecurityAccess(
                <SecurityAccessRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::TesterPresent => Self::TesterPresent(
                <TesterPresentRequest as Decode>::decode_exact(payload)?,
            ),
            UdsServiceType::TransferData => {
                Self::TransferData(<TransferDataRequest as Decode>::decode_exact(payload)?)
            }
            UdsServiceType::WriteDataByIdentifier => Self::WriteDataByIdentifier(
                <WriteDataByIdentifierRequest as Decode>::decode_exact(payload)?,
            ),
            _ => Self::Other {
                sid: buf[0],
                data: payload,
            },
        };
        Ok((request, &[]))
    }
}

impl Encode for Request<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let sid = match self {
            Self::Other { sid, .. } => *sid,
            other => other.service().to_request_sid(),
        };
        let sid_len = write_u8(writer, sid)?;
        let payload = match self {
            Self::ClearDiagnosticInfo(req) => req.encode(writer)?,
            Self::CommunicationControl(req) => req.encode(writer)?,
            Self::ControlDtcSetting(req) => req.encode(writer)?,
            Self::DiagnosticSessionControl(req) => req.encode(writer)?,
            Self::EcuReset(req) => req.encode(writer)?,
            Self::ReadDataByIdentifier(req) => req.encode(writer)?,
            Self::ReadDtcInfo(req) => req.encode(writer)?,
            Self::WriteDataByIdentifier(req) => req.encode(writer)?,
            Self::RequestDownload(req) => req.encode(writer)?,
            Self::RequestFileTransfer(req) => req.encode(writer)?,
            Self::RequestTransferExit(req) => req.encode(writer)?,
            Self::RequestUpload(req) => req.encode(writer)?,
            Self::Other { data, .. } => write_bytes(writer, data)?,
            Self::RoutineControl(req) => req.encode(writer)?,
            Self::SecurityAccess(req) => req.encode(writer)?,
            Self::TesterPresent(req) => req.encode(writer)?,
            Self::TransferData(req) => req.encode(writer)?,
        };
        Ok(sid_len + payload)
    }
}

impl Request<'_> {
    /// Whether the positive response for this request is suppressed (SPRMIB).
    ///
    /// Only a service ISO 14229-1 gives a sub-function can suppress a positive response,
    /// because the suppressPosRspMsgIndicationBit is bit 7 of that sub-function byte. A
    /// service with no sub-function answers `Some(false)`, and so does an enumerated but
    /// unmodeled one, since [`UdsServiceType::has_sub_function`] is a fact of the standard
    /// rather than of this crate's coverage. `None` means the crate cannot say: the SID is
    /// one ISO 14229-1 does not assign, the sub-function byte is absent from the payload,
    /// or the SID is the withdrawn `0x83`. `None` is kept apart from `Some(false)` because
    /// ISO 14229-2:2021 clause 10.3 gates `tP3_Client_Phys` on this bit, so a wrong `false`
    /// costs a session layer a timeout and retries for a request nobody wanted answered.
    #[must_use]
    pub fn is_positive_response_suppressed(&self) -> Option<bool> {
        match self {
            Self::CommunicationControl(req) => Some(req.suppress_positive_response),
            Self::ControlDtcSetting(req) => Some(req.suppress_positive_response),
            Self::DiagnosticSessionControl(req) => Some(req.suppress_positive_response),
            Self::EcuReset(req) => Some(req.suppress_positive_response),
            Self::ReadDtcInfo(req) => Some(req.suppress_positive_response),
            Self::RoutineControl(req) => Some(req.suppress_positive_response),
            Self::SecurityAccess(req) => Some(req.suppress_positive_response),
            Self::TesterPresent(req) => Some(req.suppress_positive_response),
            // The modeled services ISO gives no sub-function, spelled out so that a variant
            // added later has to choose an answer rather than inherit `false`.
            Self::ClearDiagnosticInfo(_)
            | Self::ReadDataByIdentifier(_)
            | Self::RequestDownload(_)
            | Self::RequestFileTransfer(_)
            | Self::RequestTransferExit(_)
            | Self::RequestUpload(_)
            | Self::TransferData(_)
            | Self::WriteDataByIdentifier(_) => Some(false),
            Self::Other { data, .. } => {
                if self.service().has_sub_function()? {
                    data.first().copied().map(|byte| split_sprmib(byte).0)
                } else {
                    Some(false)
                }
            }
        }
    }

    /// The [`NegativeResponseCode`]s ISO 14229-1 **lists** for this request's service.
    ///
    /// Dispatches to the request type's own associated function (for example
    /// [`EcuResetRequest::allowed_nack_codes`]) so a server holding a decoded [`Request`]
    /// does not re-match every variant. The slice is a floor, not a ceiling: clause 9.4
    /// says the Annex A.1 codes "shall be used **in addition to**" the per-service tables,
    /// and `0x78`
    /// [`RequestCorrectlyReceivedResponsePending`](NegativeResponseCode::RequestCorrectlyReceivedResponsePending)
    /// appears in none of them. Read it as the codes the standard tabulates for the
    /// service, for a tester UI or a conformance report, never as a validation whitelist.
    /// Empty for [`Request::Other`], meaning the set is unknown, not that no code applies.
    #[must_use]
    pub fn allowed_nack_codes(&self) -> &'static [NegativeResponseCode] {
        match self {
            Self::ClearDiagnosticInfo(_) => {
                ClearDiagnosticInfoRequest::allowed_nack_codes()
            }
            Self::CommunicationControl(_) => {
                CommunicationControlRequest::allowed_nack_codes()
            }
            Self::ControlDtcSetting(_) => ControlDtcSettingRequest::allowed_nack_codes(),
            Self::DiagnosticSessionControl(_) => {
                DiagnosticSessionControlRequest::allowed_nack_codes()
            }
            Self::EcuReset(_) => EcuResetRequest::allowed_nack_codes(),
            Self::ReadDataByIdentifier(_) => {
                ReadDataByIdentifierRequest::allowed_nack_codes()
            }
            Self::ReadDtcInfo(_) => ReadDtcInfoRequest::allowed_nack_codes(),
            Self::RequestDownload(_) => RequestDownloadRequest::allowed_nack_codes(),
            Self::RequestFileTransfer(_) => {
                RequestFileTransferRequest::allowed_nack_codes()
            }
            Self::RequestTransferExit(_) => {
                RequestTransferExitRequest::allowed_nack_codes()
            }
            Self::RequestUpload(_) => RequestUploadRequest::allowed_nack_codes(),
            Self::RoutineControl(_) => RoutineControlRequest::allowed_nack_codes(),
            Self::SecurityAccess(_) => SecurityAccessRequest::allowed_nack_codes(),
            Self::TesterPresent(_) => TesterPresentRequest::allowed_nack_codes(),
            Self::TransferData(_) => TransferDataRequest::allowed_nack_codes(),
            Self::WriteDataByIdentifier(_) => {
                WriteDataByIdentifierRequest::allowed_nack_codes()
            }
            Self::Other { .. } => &[],
        }
    }

    /// Returns the [`UdsServiceType`] corresponding to this request variant.
    #[must_use]
    pub fn service(&self) -> UdsServiceType {
        match self {
            Self::ClearDiagnosticInfo(_) => UdsServiceType::ClearDiagnosticInfo,
            Self::CommunicationControl(_) => UdsServiceType::CommunicationControl,
            Self::ControlDtcSetting(_) => UdsServiceType::ControlDtcSetting,
            Self::DiagnosticSessionControl(_) => UdsServiceType::DiagnosticSessionControl,
            Self::EcuReset(_) => UdsServiceType::EcuReset,
            Self::ReadDataByIdentifier(_) => UdsServiceType::ReadDataByIdentifier,
            Self::ReadDtcInfo(_) => UdsServiceType::ReadDtcInfo,
            Self::RequestDownload(_) => UdsServiceType::RequestDownload,
            Self::RequestFileTransfer(_) => UdsServiceType::RequestFileTransfer,
            Self::RequestTransferExit(_) => UdsServiceType::RequestTransferExit,
            Self::RequestUpload(_) => UdsServiceType::RequestUpload,
            Self::RoutineControl(_) => UdsServiceType::RoutineControl,
            Self::SecurityAccess(_) => UdsServiceType::SecurityAccess,
            Self::TesterPresent(_) => UdsServiceType::TesterPresent,
            Self::TransferData(_) => UdsServiceType::TransferData,
            Self::WriteDataByIdentifier(_) => UdsServiceType::WriteDataByIdentifier,
            Self::Other { sid, .. } => UdsServiceType::from_request_sid(*sid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResetType, service::UdsServiceType};

    #[test]
    fn decode_rejects_trailing_bytes() {
        // ECU reset is a fixed 1-byte payload; an extra trailing byte is a
        // malformed frame and must be rejected rather than silently dropped.
        let mut frame = [0u8; 3];
        frame[0] = UdsServiceType::EcuReset.to_request_sid();
        frame[1] = u8::from(ResetType::HardReset);
        frame[2] = 0xAA; // trailing junk
        let result = Request::decode(&frame);
        assert!(matches!(result, Err(Error::TrailingBytes(_))));
    }

    #[test]
    fn suppression_forwards_to_inner_request() {
        let suppressed =
            Request::EcuReset(EcuResetRequest::new(true, ResetType::HardReset));
        assert_eq!(suppressed.is_positive_response_suppressed(), Some(true));

        let not_suppressed =
            Request::EcuReset(EcuResetRequest::new(false, ResetType::HardReset));
        assert_eq!(
            not_suppressed.is_positive_response_suppressed(),
            Some(false)
        );
    }

    #[test]
    fn suppression_is_read_from_the_sub_function_of_an_unmodeled_service() {
        // 0x2C DynamicallyDefineDataIdentifier is enumerated but unmodeled, so it decodes
        // to `Other`. It does have a sub-function, so bit 7 of the first payload byte is
        // its SPRMIB and the crate can answer even though it does not model the payload.
        // Sub-function 0x01 defineByIdentifier, with and without the bit.
        let (suppressed, _) = Request::decode(&[0x2C, 0x81, 0xF3, 0x00]).unwrap();
        assert!(matches!(suppressed, Request::Other { .. }));
        assert_eq!(suppressed.is_positive_response_suppressed(), Some(true));

        let (not_suppressed, _) = Request::decode(&[0x2C, 0x01, 0xF3, 0x00]).unwrap();
        assert_eq!(
            not_suppressed.is_positive_response_suppressed(),
            Some(false)
        );
    }

    #[test]
    fn an_unmodeled_service_without_a_sub_function_is_never_suppressed() {
        // 0x23 ReadMemoryByAddress is enumerated but unmodeled, and ISO gives it no
        // sub-function -- so there is no SPRMIB anywhere in the frame and the answer is a
        // definite `Some(false)`, whatever the payload bytes happen to be. 0xAA has bit 7
        // set, which would read as "suppressed" if the payload were mistaken for a
        // sub-function.
        let (req, _) = Request::decode(&[0x23, 0xAA, 0xBB]).unwrap();
        assert!(matches!(req, Request::Other { .. }));
        assert_eq!(req.is_positive_response_suppressed(), Some(false));
    }

    #[test]
    fn suppression_is_unknown_for_a_vendor_specific_service() {
        // 0x40 is not in the ISO request table, so the crate has no basis for an answer.
        // This is the case the `Option` exists for: a `false` here would make a
        // fire-and-forget vendor request look response-expected to a session layer.
        let (req, _) = Request::decode(&[0x40, 0xAA, 0xBB]).unwrap();
        assert!(matches!(req, Request::Other { .. }));
        assert_eq!(req.is_positive_response_suppressed(), None);
    }

    #[test]
    fn suppression_is_unknown_when_the_sub_function_byte_is_absent() {
        // 0x2C has a sub-function, but this frame carries no payload -- so the byte the
        // SPRMIB lives in is not on the wire. The frame is malformed for the service;
        // either way there is no bit to report, so the answer is unknown rather than
        // `Some(false)`.
        let (req, _) = Request::decode(&[0x2C]).unwrap();
        assert!(matches!(req, Request::Other { data, .. } if data.is_empty()));
        assert_eq!(req.is_positive_response_suppressed(), None);
    }

    #[test]
    fn write_data_by_identifier_request_roundtrips() {
        // SID 0x2E, DID 0xF190, one data byte 0x01
        let wire = [0x2E, 0xF1, 0x90, 0x01];
        let (req, rest) = Request::decode(&wire).unwrap();
        assert_eq!(rest, [0u8; 0]);
        assert!(matches!(req, Request::WriteDataByIdentifier(_)));
        let mut buf = [0u8; 8];
        let written =
            Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &wire);
    }

    #[test]
    fn routine_control_request_roundtrips_with_suppress_bit() {
        // SID 0x31, sub 0x81 (StartRoutine + SPRMIB), RID 0xFF00, param 0xAA
        let wire = [0x31, 0x81, 0xFF, 0x00, 0xAA];
        let (req, rest) = Request::decode(&wire).unwrap();
        assert_eq!(rest, [0u8; 0]);
        assert_eq!(req.is_positive_response_suppressed(), Some(true));
        let mut buf = [0u8; 8];
        let written =
            Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &wire);
    }

    /// One minimal-but-valid frame per modeled service, paired with the NRC table that
    /// service dispatches to. Shared by the two tests below so they cannot drift apart.
    ///
    /// Every service with a sub-function has bit 7 of that byte set: with the bit clear
    /// every service answers `Some(false)` and a flipped sub-function table entry goes
    /// unnoticed. Two pairs share a table, `CommunicationControl` with `ControlDTCSetting`
    /// and `RequestDownload` with `RequestUpload` (Tables 444 and 449), so swapping either
    /// pair's rows is unobservable.
    fn modeled_frames() -> [(&'static [u8], &'static [NegativeResponseCode]); 16] {
        [
            // ClearDiagnosticInfo, no sub-function
            (
                &[0x14, 0xFF, 0xFF, 0xFF, 0x00],
                ClearDiagnosticInfoRequest::allowed_nack_codes(),
            ),
            // CommunicationControl, EnableRxAndTx + SPRMIB
            (
                &[0x28, 0x80, 0x01],
                CommunicationControlRequest::allowed_nack_codes(),
            ),
            // ControlDtcSetting, On + SPRMIB
            (
                &[0x85, 0x81],
                ControlDtcSettingRequest::allowed_nack_codes(),
            ),
            // DiagnosticSessionControl, DefaultSession + SPRMIB
            (
                &[0x10, 0x81],
                DiagnosticSessionControlRequest::allowed_nack_codes(),
            ),
            // EcuReset, HardReset + SPRMIB
            (&[0x11, 0x81], EcuResetRequest::allowed_nack_codes()),
            // ReadDataByIdentifier, no sub-function
            (
                &[0x22, 0xF1, 0x90],
                ReadDataByIdentifierRequest::allowed_nack_codes(),
            ),
            // ReadDtcInfo, ReportDtcByStatusMask + SPRMIB
            (
                &[0x19, 0x82, 0xFF],
                ReadDtcInfoRequest::allowed_nack_codes(),
            ),
            // RequestDownload, no sub-function
            (
                &[0x34, 0x00, 0x12, 0xBE, 0xEF, 0x10],
                RequestDownloadRequest::allowed_nack_codes(),
            ),
            // RequestFileTransfer, no sub-function
            (
                &[0x38, 0x02, 0x00, 0x01, b'a'],
                RequestFileTransferRequest::allowed_nack_codes(),
            ),
            // RequestUpload, no sub-function
            (
                &[0x35, 0x00, 0x12, 0xBE, 0xEF, 0x10],
                RequestUploadRequest::allowed_nack_codes(),
            ),
            // RequestTransferExit, no sub-function
            (&[0x37], RequestTransferExitRequest::allowed_nack_codes()),
            // RoutineControl, StartRoutine + SPRMIB
            (
                &[0x31, 0x81, 0xFF, 0x00],
                RoutineControlRequest::allowed_nack_codes(),
            ),
            // SecurityAccess, RequestSeed + SPRMIB
            (
                &[0x27, 0x81, 0xAA],
                SecurityAccessRequest::allowed_nack_codes(),
            ),
            // TesterPresent, ZeroSubFunction + SPRMIB
            (&[0x3E, 0x80], TesterPresentRequest::allowed_nack_codes()),
            // TransferData, no sub-function
            (
                &[0x36, 0x01, 0xAA],
                TransferDataRequest::allowed_nack_codes(),
            ),
            // WriteDataByIdentifier, no sub-function
            (
                &[0x2E, 0xF1, 0x90, 0x01],
                WriteDataByIdentifierRequest::allowed_nack_codes(),
            ),
        ]
    }

    #[test]
    fn allowed_nack_codes_dispatches_for_every_modeled_variant() {
        // Each frame is compared against the inherent table it dispatches to, not merely
        // against `!is_empty()`: every modeled service has a non-empty table, so an arm
        // returning another service's set would otherwise pass.
        let frames = modeled_frames();
        for (frame, expected) in frames {
            let (req, _) = Request::decode(frame).unwrap_or_else(|e| {
                panic!("frame {frame:02X?} should decode, got {e:?}");
            });
            assert!(
                !matches!(req, Request::Other { .. }),
                "frame {frame:02X?} decoded to Other; the table needs updating"
            );
            assert_eq!(
                req.allowed_nack_codes(),
                expected,
                "{:?} dispatched to the wrong NRC table",
                req.service()
            );
        }
    }

    #[test]
    fn every_modeled_variant_agrees_with_the_sub_function_table() {
        // `has_sub_function` and the match in `is_positive_response_suppressed` are two
        // copies of one ISO fact. This ties them together as a biconditional: the table
        // says a service has a sub-function exactly when the dispatch reports a set SPRMIB
        // for a frame that sets one, which every `modeled_frames` entry does.
        for (frame, _) in modeled_frames() {
            let (req, _) = Request::decode(frame).unwrap_or_else(|e| {
                panic!("frame {frame:02X?} should decode, got {e:?}");
            });
            assert!(
                !matches!(req, Request::Other { .. }),
                "frame {frame:02X?} decoded to Other; the table needs updating"
            );
            let service = req.service();
            if service.has_sub_function() == Some(true) {
                assert!(
                    frame[1] & 0x80 != 0,
                    "frame {frame:02X?} for {service:?} does not set bit 7 of the sub-function \
                     byte; `modeled_frames` must set the SPRMIB on every sub-function service, \
                     or this test cannot detect a flipped sub-function table entry"
                );
            }
            let suppressed = req.is_positive_response_suppressed();
            assert!(
                suppressed.is_some(),
                "{service:?} is modeled, so its SPRMIB is never unknown"
            );
            // `Some(true)` for the eight services with a sub-function, `Some(false)` for
            // the eight without; a table entry flipped either way breaks this.
            assert_eq!(
                suppressed,
                service.has_sub_function(),
                "{service:?}: dispatch and sub-function table disagree"
            );
        }
    }

    #[test]
    fn allowed_nack_codes_agrees_with_the_inherent_method() {
        let (req, _) = Request::decode(&[0x11, 0x01]).unwrap();
        assert_eq!(
            req.allowed_nack_codes(),
            EcuResetRequest::allowed_nack_codes()
        );
    }

    #[test]
    fn allowed_nack_codes_is_empty_for_pass_through() {
        // 0x23 ReadMemoryByAddress is enumerated but unmodeled, so the crate has no NRC
        // table for it. An empty slice says "unknown", not "none apply".
        let (req, _) = Request::decode(&[0x23, 0xAA]).unwrap();
        assert!(matches!(req, Request::Other { .. }));
        assert_eq!(req.allowed_nack_codes(), []);
    }

    #[test]
    fn unmodeled_service_decodes_to_other() {
        // 0x23 = ReadMemoryByAddress, enumerated but not modeled.
        let frame = [0x23, 0xAA, 0xBB];
        let (req, rest) = Request::decode(&frame).unwrap();
        assert_eq!(rest, [0u8; 0]);
        match req {
            Request::Other { sid, data } => {
                assert_eq!(sid, 0x23);
                assert_eq!(data, &[0xAA, 0xBB]);
            }
            other => panic!("expected Other, got {other:?}"),
        }
        let mut buf = [0u8; 8];
        let written =
            Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &frame);
    }

    #[test]
    fn unknown_request_byte_round_trips_losslessly() {
        // 0x40 is not in the ISO request table; it survives a decode, encode round trip.
        let frame = [0x40, 0xAA, 0xBB];
        let (req, rest) = Request::decode(&frame).unwrap();
        assert_eq!(rest, [0u8; 0]);
        match req {
            Request::Other { sid, data } => {
                assert_eq!(sid, 0x40);
                assert_eq!(data, &[0xAA, 0xBB]);
            }
            other => panic!("expected Other, got {other:?}"),
        }
        let mut buf = [0u8; 8];
        let written =
            Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &frame);
    }
}
