//! The sans-io halves of a client exchange (``UDSSVC_ARCH_0028``): encoding a request
//! into the request buffer, and reading what came back.
//!
//! ``UDSSVC_ARCH_0020`` — the service identifiers and the negative response codes are
//! `uds_protocol`'s; the bytes between them are this vocabulary's.

use super::{Answer, MalformedResponse, Records, Response};
use crate::{DataIdentifier, DiagnosticSessionType, SessionTiming, UdsServiceType};
use crate::{RoutineIdentifier, SecurityLevel};
use uds_protocol::{
    ClearDiagnosticInfoRequest, CommunicationControlRequest, CommunicationControlType,
    CommunicationType, ControlDtcSettingRequest, Decode, DtcRecord, DtcSettingType,
    EcuResetRequest, Encode, NegativeResponseCode, ReadDtcInfoRequest,
    ReadDtcInfoSubFunction, Request, ResetType, RoutineControlRequest,
    RoutineControlSubFunction, SecurityAccessRequest, SecurityAccessType, SubnetNumber,
    WriteDataByIdentifierRequest,
};
use uds_session::Address;
use uds_session::{ClientRx, SessionSelection, Solicitation};

/// The negative response service identifier (8.5 Tables 2 and 3).
const NEGATIVE: u8 = 0x7F;
/// The offset from a request's service identifier to its positive response's.
const POSITIVE_OFFSET: u8 = 0x40;
/// `requestCorrectlyReceived-ResponsePending`.
const PENDING: u8 = 0x78;

/// The suppressed `TesterPresent` a keep-alive sends (10.7; the suppress bit, 9.2.2).
pub(super) const KEEP_ALIVE: [u8; 2] = [0x3E, 0x80];

/// The service identifier of `service`.
const fn sid(service: UdsServiceType) -> u8 {
    service.to_request_sid()
}

/// Encode a `ReadDataByIdentifier` request for `identifiers` into `request`.
///
/// `None` where `identifiers` is empty, names more than `max_dids` (the
/// `max_dids_per_request` given to [`crate::uds_client`]), or names more than `request`
/// holds.
pub(super) fn read_data_by_identifier<D: DataIdentifier>(
    request: &mut [u8],
    identifiers: &[D],
    max_dids: usize,
) -> Option<usize> {
    if identifiers.is_empty() || identifiers.len() > max_dids {
        return None;
    }
    let (head, body) = request.split_first_mut()?;
    *head = sid(UdsServiceType::ReadDataByIdentifier);
    let (slots, _) = body.as_chunks_mut::<2>();
    for (slot, did) in slots
        .get_mut(..identifiers.len())?
        .iter_mut()
        .zip(identifiers)
    {
        *slot = did.as_u16().to_be_bytes();
    }
    identifiers.len().checked_mul(2)?.checked_add(1)
}

/// Encode a `CommunicationControl` request into `buffer`. `None` where `node_id`
/// disagrees with `control`: a plain type takes none, an enhanced-address type needs
/// one.
pub(super) fn communication_control(
    buffer: &mut [u8],
    control: CommunicationControlType,
    communication: CommunicationType,
    subnet: SubnetNumber,
    node_id: Option<u16>,
) -> Option<usize> {
    let built = match node_id {
        Some(node) => CommunicationControlRequest::new_with_node_id(
            false,
            control,
            communication,
            node,
        ),
        None => CommunicationControlRequest::new(false, control, communication),
    };
    let control = built.ok()?.with_subnet(subnet);
    request(buffer, &Request::CommunicationControl(control))
}

/// Encode a `ControlDTCSetting` request into `buffer`.
pub(super) fn control_dtc_setting(
    buffer: &mut [u8],
    setting: DtcSettingType,
    option_record: &[u8],
) -> Option<usize> {
    let control =
        ControlDtcSettingRequest::new_with_option_record(false, setting, option_record);
    request(buffer, &Request::ControlDtcSetting(control))
}

/// Encode `request`, service identifier first, into `buffer`: its length, or `None`
/// where it does not fit, so nothing is sent.
pub(super) fn request(buffer: &mut [u8], request: &Request<'_>) -> Option<usize> {
    request.encode_to_slice(buffer).ok()
}

/// Encode a `WriteDataByIdentifier` request for `did` with `record` into `buffer`.
///
/// `None` for an empty record, which Table 218 makes mandatory, or one that does not
/// fit.
pub(super) fn write_data_by_identifier<D: DataIdentifier>(
    buffer: &mut [u8],
    did: D,
    record: &[u8],
) -> Option<usize> {
    let write = WriteDataByIdentifierRequest::new(did.as_u16(), record).ok()?;
    request(buffer, &Request::WriteDataByIdentifier(write))
}

/// Encode an `ECUReset` request for `kind` into `buffer`.
pub(super) fn ecu_reset(buffer: &mut [u8], kind: ResetType) -> Option<usize> {
    request(
        buffer,
        &Request::EcuReset(EcuResetRequest::new(false, kind)),
    )
}

/// The access type for `sub_function`, one of a level's two, which cannot fail on the
/// range [`SecurityLevel`] admits.
fn access_type(sub_function: u8) -> Option<SecurityAccessType> {
    SecurityAccessType::try_from(sub_function).ok()
}

/// Encode a seed request for `level` into `buffer`: no record, no suppress bit.
pub(super) fn security_access_seed(
    buffer: &mut [u8],
    level: SecurityLevel,
) -> Option<usize> {
    let access = access_type(level.request_seed())?;
    request(
        buffer,
        &Request::SecurityAccess(SecurityAccessRequest::new(false, access, &[])),
    )
}

/// Encode a send-key request for `level` with `key` into `buffer`.
pub(super) fn security_access_key(
    buffer: &mut [u8],
    level: SecurityLevel,
    key: &[u8],
) -> Option<usize> {
    let access = access_type(level.send_key())?;
    request(
        buffer,
        &Request::SecurityAccess(SecurityAccessRequest::new(false, access, key)),
    )
}

/// Encode a `RoutineControl` request into `buffer`.
pub(super) fn routine_control<R: RoutineIdentifier>(
    buffer: &mut [u8],
    control: RoutineControlSubFunction,
    routine: R,
    option_record: &[u8],
) -> Option<usize> {
    let control =
        RoutineControlRequest::new(false, control, routine.as_u16(), option_record);
    request(buffer, &Request::RoutineControl(control))
}

/// Encode a `ReadDTCInformation` request for `report` into `buffer`.
pub(super) fn read_dtc_information(
    buffer: &mut [u8],
    report: ReadDtcInfoSubFunction,
) -> Option<usize> {
    request(
        buffer,
        &Request::ReadDtcInfo(ReadDtcInfoRequest::new(false, report)),
    )
}

/// Encode a `ClearDiagnosticInformation` request into `buffer`.
pub(super) fn clear_diagnostic_information(
    buffer: &mut [u8],
    group: DtcRecord,
    memory_selection: Option<u8>,
) -> Option<usize> {
    let clear = match memory_selection {
        Some(memory) => {
            ClearDiagnosticInfoRequest::new_with_memory_selection(group, memory)
        }
        None => ClearDiagnosticInfoRequest::new(group),
    };
    request(buffer, &Request::ClearDiagnosticInfo(clear))
}

/// Encode a `DiagnosticSessionControl` request for `session` into `request`.
pub(super) fn diagnostic_session_control(
    request: &mut [u8],
    session: DiagnosticSessionType,
) -> Option<usize> {
    let encoded = [
        sid(UdsServiceType::DiagnosticSessionControl),
        u8::from(session),
    ];
    request.get_mut(..encoded.len())?.copy_from_slice(&encoded);
    Some(encoded.len())
}

/// How a message from a server answers the request `expected` describes: its service
/// identifier, and the byte a positive response to it echoes after its own, where the
/// service has one.
///
/// A final response echoing the service answers the request (8.5, 8.6), and
/// a positive one echoing another value of that byte does not. Anything else is
/// classified unsolicited, so it closes no response window. That is this client's own
/// policy: by ``UDSS_LLR_0065`` a late reply to an earlier request is solicited, but
/// taking it for the current request's answer would hand the caller the wrong one. A late
/// reply to the same service, echoing the same byte, cannot be told apart this way.
/// `session` is the selection a positive answer carries.
pub(super) fn classify(
    expected: Option<(u8, Option<u8>)>,
    message: &[u8],
    session: Option<SessionSelection>,
) -> ClientRx {
    let unsolicited = ClientRx::FinalResponse {
        solicitation: Solicitation::Unsolicited,
        session: None,
    };
    let Some((sid, echo)) = expected else {
        return unsolicited;
    };
    let echoes = |rest: &[u8]| echo.is_none_or(|e| rest.first().is_none_or(|b| *b == e));
    match message {
        [NEGATIVE, s, PENDING, ..] if *s == sid => ClientRx::ResponsePending,
        [NEGATIVE, s, ..] if *s == sid => ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: None,
        },
        [p, rest @ ..] if Some(*p) == sid.checked_add(POSITIVE_OFFSET) && echoes(rest) => {
            ClientRx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session,
            }
        }
        _ => unsolicited,
    }
}

/// Whether `message` is the positive response to `service`, whatever follows its
/// service identifier.
pub(super) fn is_positive(service: UdsServiceType, message: &[u8]) -> bool {
    message.first().copied() == Some(service.to_response_sid())
}

/// Whether `class` is a final response answering the request it was classified against.
pub(super) const fn solicited_final(class: ClientRx) -> bool {
    matches!(
        class,
        ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            ..
        }
    )
}

/// Whether a message arrived whole or longer than the response buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arrived {
    /// It fit.
    Whole,
    /// It did not ([`crate::TransportEvent::DataTooLong`]), and was this long where the
    /// transport knew.
    TooLong(Option<usize>),
}

/// What a final response said, before its positive bytes are read.
#[derive(Debug, Clone, Copy)]
pub(super) enum Final<'d> {
    /// The bytes after the positive response's service identifier.
    Positive(&'d [u8]),
    /// A negative response's code.
    Negative(NegativeResponseCode),
    /// Neither could be read.
    Malformed(MalformedResponse),
}

/// Split a final response [`classify`] found solicited.
///
/// A response longer than the buffer is never read as records, because a cut at a
/// record boundary would read as a shorter answer.
pub(super) fn final_response(message: &[u8], arrived: Arrived) -> Final<'_> {
    if let Arrived::TooLong(declared) = arrived {
        return Final::Malformed(MalformedResponse::Overlong { declared });
    }
    match message {
        [NEGATIVE, _, code, ..] => Final::Negative(NegativeResponseCode::from(*code)),
        [NEGATIVE, ..] | [] => Final::Malformed(MalformedResponse::Short),
        [_, rest @ ..] => Final::Positive(rest),
    }
}

/// A final response read as `R`, exactly: one the decoder cannot read whole is
/// malformed, short where it ran out of bytes.
pub(super) fn decoded<'d, R>(answer: Final<'d>) -> Response<R>
where
    R: Decode<'d, Error = uds_protocol::Error>,
{
    match answer {
        Final::Positive(rest) => match R::decode_exact(rest) {
            Ok(value) => Response::Positive(value),
            Err(uds_protocol::Error::InsufficientData(_)) => {
                Response::Malformed(MalformedResponse::Short)
            }
            Err(_) => Response::Malformed(MalformedResponse::Decode),
        },
        Final::Negative(code) => Response::Negative(code),
        Final::Malformed(error) => Response::Malformed(error),
    }
}

/// A `ReadDataByIdentifier` response, read against this vocabulary.
pub(super) fn records<D: DataIdentifier>(answer: Final<'_>) -> Response<Records<'_, D>> {
    match answer {
        Final::Positive(rest) => match Records::validate(rest) {
            Ok(records) => Response::Positive(records),
            Err(error) => Response::Malformed(error),
        },
        Final::Negative(code) => Response::Negative(code),
        Final::Malformed(error) => Response::Malformed(error),
    }
}

/// One server's answer to a functional `ReadDataByIdentifier`, read against this
/// vocabulary.
pub(super) fn answer<D: DataIdentifier>(from: Address, answer: Final<'_>) -> Answer<'_, D> {
    match answer {
        Final::Positive(rest) => match Records::validate(rest) {
            Ok(records) => Answer::Positive { from, records },
            Err(error) => Answer::Malformed { from, error },
        },
        Final::Negative(code) => Answer::Negative { from, code },
        Final::Malformed(error) => Answer::Malformed { from, error },
    }
}

/// A `DiagnosticSessionControl` response: the session echo, then ISO 14229-1:2020
/// Table 29's `P2Server_max` in 1 ms units and `P2*Server_max` in 10 ms units.
pub(super) fn session_timing(answer: Final<'_>) -> Response<SessionTiming> {
    match answer {
        Final::Positive([_session, p2_hi, p2_lo, star_hi, star_lo, ..]) => {
            Response::Positive(SessionTiming {
                p2_server_max_ms: u16::from_be_bytes([*p2_hi, *p2_lo]),
                p2_star_server_max_10ms: u16::from_be_bytes([*star_hi, *star_lo]),
            })
        }
        Final::Positive(_) => Response::Malformed(MalformedResponse::Short),
        Final::Negative(code) => Response::Negative(code),
        Final::Malformed(error) => Response::Malformed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arrived, Final, classify, decoded, final_response, read_data_by_identifier,
        request, session_timing,
    };
    use crate::client::{MalformedResponse, Response};
    use crate::{DataIdentifier, RecordError, SessionTiming};
    use uds_protocol::NegativeResponseCode;
    use uds_session::{ClientRx, SessionSelection, Solicitation};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Did(u16);
    impl DataIdentifier for Did {
        const MAX_RECORD_LEN: usize = 1;
        fn as_u16(self) -> u16 {
            self.0
        }
        fn from_u16(v: u16) -> Option<Self> {
            Some(Self(v))
        }
        fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
            buf.split_at_checked(1).ok_or(RecordError::Short)
        }
    }

    /// ``UDSSVC_ARCH_0020`` — the request is the service identifier and each identifier
    /// big-endian, and one naming no identifier, more than the declared maximum, or more
    /// than the buffer holds is refused before anything is sent.
    #[test]
    fn a_read_encodes_its_identifiers_big_endian_and_refuses_what_does_not_fit() {
        let mut request = [0; 7];
        assert_eq!(
            read_data_by_identifier(&mut request, &[Did(0xF40D), Did(0xF190)], 2),
            Some(5)
        );
        assert_eq!(request.get(..5), Some(&[0x22, 0xF4, 0x0D, 0xF1, 0x90][..]));
        assert_eq!(read_data_by_identifier::<Did>(&mut request, &[], 2), None);
        assert_eq!(
            read_data_by_identifier(&mut request, &[Did(1), Did(2), Did(3)], 2),
            None
        );
        assert_eq!(
            read_data_by_identifier(&mut request, &[Did(1), Did(2), Did(3), Did(4)], 4),
            None
        );
    }

    /// 8.5, 8.6 — only a reply echoing the request's service identifier answers it, and a
    /// positive one only where it echoes the request's sub-function too; a stale reply to
    /// another service or session is unsolicited, and a 0x78 is pending.
    #[test]
    fn a_response_answers_the_request_only_where_it_echoes_its_service() {
        let solicited = ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: None,
        };
        let unsolicited = ClientRx::FinalResponse {
            solicitation: Solicitation::Unsolicited,
            session: None,
        };
        assert_eq!(
            classify(Some((0x22, None)), &[0x62, 0xF4, 0x0D, 0x40], None),
            solicited
        );
        assert_eq!(
            classify(Some((0x22, None)), &[0x7F, 0x22, 0x31], None),
            solicited
        );
        assert_eq!(
            classify(Some((0x22, None)), &[0x7F, 0x22, 0x78], None),
            ClientRx::ResponsePending
        );
        assert_eq!(
            classify(Some((0x22, None)), &[0x50, 0x03], None),
            unsolicited
        );
        assert_eq!(
            classify(Some((0x22, None)), &[0x7F, 0x10, 0x78], None),
            unsolicited
        );
        assert_eq!(classify(None, &[0x62, 0xF4, 0x0D, 0x40], None), unsolicited);
        assert_eq!(
            classify(
                Some((0x10, Some(0x03))),
                &[0x50, 0x03],
                Some(SessionSelection::NonDefault)
            ),
            ClientRx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: Some(SessionSelection::NonDefault),
            }
        );
        assert_eq!(
            classify(Some((0x10, Some(0x03))), &[0x50, 0x02], None),
            unsolicited
        );
        assert_eq!(
            classify(Some((0x27, Some(0x01))), &[0x67, 0x02, 0x00, 0x00], None),
            unsolicited
        );
    }

    /// A request is the service identifier then the struct's own bytes, and one the
    /// buffer cannot hold is refused before anything is sent.
    #[test]
    fn a_request_is_encoded_whole_or_not_at_all() {
        use uds_protocol::{EcuResetRequest, Request, ResetType};
        let reset = Request::EcuReset(EcuResetRequest::new(false, ResetType::HardReset));
        let mut buffer = [0; 2];
        assert_eq!(request(&mut buffer, &reset), Some(2));
        assert_eq!(buffer, [0x11, 0x01]);
        let mut short = [0; 1];
        assert_eq!(request(&mut short, &reset), None);
    }

    /// A positive reply is decoded exactly; a short one is `Short`, trailing bytes are
    /// `Decode`, and a negative reply is its code.
    #[test]
    fn a_reply_is_decoded_exactly() {
        use uds_protocol::{EcuResetResponse, ResetType};
        let whole =
            decoded::<EcuResetResponse>(final_response(&[0x51, 0x01], Arrived::Whole));
        assert_eq!(
            whole,
            Response::Positive(EcuResetResponse::new(ResetType::HardReset))
        );
        assert_eq!(
            decoded::<EcuResetResponse>(final_response(&[0x51], Arrived::Whole)),
            Response::Malformed(MalformedResponse::Short)
        );
        assert_eq!(
            decoded::<EcuResetResponse>(final_response(
                &[0x51, 0x01, 0x05, 0x06],
                Arrived::Whole
            )),
            Response::Malformed(MalformedResponse::Decode)
        );
        assert_eq!(
            decoded::<EcuResetResponse>(final_response(
                &[0x7F, 0x11, 0x22],
                Arrived::Whole
            )),
            Response::Negative(NegativeResponseCode::from(0x22))
        );
    }

    /// ISO 14229-1:2020 Table 29 — `P2*Server_max` is read in 10 ms units:
    /// `0x01F4` is five seconds, kept in the wire unit the type names.
    #[test]
    fn p2_star_is_read_in_ten_millisecond_units() {
        let response = [0x50, 0x03, 0x00, 0x32, 0x01, 0xF4];
        assert_eq!(
            session_timing(final_response(&response, Arrived::Whole)),
            Response::Positive(SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max_10ms: 500,
            })
        );
        assert!(matches!(
            final_response(&response, Arrived::TooLong(Some(9))),
            Final::Malformed(MalformedResponse::Overlong { declared: Some(9) })
        ));
    }
}
