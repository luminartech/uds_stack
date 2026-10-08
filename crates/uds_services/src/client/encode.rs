//! The sans-io halves of a client exchange (``UDSSVC_ARCH_0028``): encoding a request
//! into the request buffer, and reading what came back.
//!
//! ``UDSSVC_ARCH_0020`` — the service identifiers and the negative response codes are
//! `uds_protocol`'s; the bytes between them are this vocabulary's.

use super::{Answer, Records, Response};
use crate::{
    DataIdentifier, DiagnosticSessionType, RecordError, SessionTiming, UdsServiceType,
};
use uds_protocol::NegativeResponseCode;
use uds_session::Address;
use uds_session::{ClientRx, SessionSelection, Solicitation};

/// The negative response service identifier (ISO 14229-1:2020 Table A.1).
const NEGATIVE: u8 = 0x7F;
/// The offset from a request's service identifier to its positive response's.
const POSITIVE_OFFSET: u8 = 0x40;
/// `requestCorrectlyReceived-ResponsePending`.
const PENDING: u8 = 0x78;

/// The suppressed `TesterPresent` a keep-alive sends (ISO 14229-1:2020 10.5).
pub(super) const KEEP_ALIVE: [u8; 2] = [0x3E, 0x80];

/// The service identifier of `service`.
const fn sid(service: UdsServiceType) -> u8 {
    service.to_request_sid()
}

/// Encode a `ReadDataByIdentifier` request for `identifiers` into `request`.
///
/// `None` where `identifiers` is empty, or names more than `request` holds — which is
/// more than the `max_dids_per_request` [`crate::uds_client`] folded it from.
pub(super) fn read_data_by_identifier<D: DataIdentifier>(
    request: &mut [u8],
    identifiers: &[D],
) -> Option<usize> {
    if identifiers.is_empty() {
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

/// How a message from a server answers the request whose service identifier is `sid`.
///
/// ``UDSS_LLR_0071`` — a final response echoing `sid` answers the request; anything else
/// was sent for some other reason, so it is unsolicited and closes no response window. A
/// stale reply to an earlier request for another service is told apart this way; one
/// for the same service cannot be. `session` is the selection a positive answer carries.
pub(super) fn classify(
    sid: Option<u8>,
    message: &[u8],
    session: Option<SessionSelection>,
) -> ClientRx {
    let unsolicited = ClientRx::FinalResponse {
        solicitation: Solicitation::Unsolicited,
        session: None,
    };
    let Some(sid) = sid else {
        return unsolicited;
    };
    match message {
        [NEGATIVE, s, PENDING, ..] if *s == sid => ClientRx::ResponsePending,
        [NEGATIVE, s, ..] if *s == sid => ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: None,
        },
        [p, ..] if Some(*p) == sid.checked_add(POSITIVE_OFFSET) => {
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
    Malformed(RecordError),
}

/// Split a final response [`classify`] found solicited.
///
/// A response longer than the buffer is never read as records, because a cut at a
/// record boundary would read as a shorter answer.
pub(super) fn final_response(message: &[u8], arrived: Arrived) -> Final<'_> {
    if let Arrived::TooLong(declared) = arrived {
        return Final::Malformed(RecordError::Overlong { declared });
    }
    match message {
        [NEGATIVE, _, code, ..] => Final::Negative(NegativeResponseCode::from(*code)),
        [NEGATIVE, ..] | [] => Final::Malformed(RecordError::Short),
        [_, rest @ ..] => Final::Positive(rest),
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
        Final::Positive(_) => Response::Malformed(RecordError::Short),
        Final::Negative(code) => Response::Negative(code),
        Final::Malformed(error) => Response::Malformed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arrived, Final, classify, final_response, read_data_by_identifier, session_timing,
    };
    use crate::client::Response;
    use crate::{DataIdentifier, RecordError, SessionTiming};
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
    /// big-endian, and one naming no identifier or more than the buffer holds is refused
    /// before anything is sent.
    #[test]
    fn a_read_encodes_its_identifiers_big_endian_and_refuses_what_does_not_fit() {
        let mut request = [0; 5];
        assert_eq!(
            read_data_by_identifier(&mut request, &[Did(0xF40D), Did(0xF190)]),
            Some(5)
        );
        assert_eq!(request, [0x22, 0xF4, 0x0D, 0xF1, 0x90]);
        assert_eq!(read_data_by_identifier::<Did>(&mut request, &[]), None);
        assert_eq!(
            read_data_by_identifier(&mut request, &[Did(1), Did(2), Did(3)]),
            None
        );
    }

    /// ``UDSS_LLR_0071`` — only a reply echoing the request's service identifier answers
    /// it; a stale reply to another service is unsolicited, and a 0x78 is pending.
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
            classify(Some(0x22), &[0x62, 0xF4, 0x0D, 0x40], None),
            solicited
        );
        assert_eq!(classify(Some(0x22), &[0x7F, 0x22, 0x31], None), solicited);
        assert_eq!(
            classify(Some(0x22), &[0x7F, 0x22, 0x78], None),
            ClientRx::ResponsePending
        );
        assert_eq!(classify(Some(0x22), &[0x50, 0x03], None), unsolicited);
        assert_eq!(classify(Some(0x22), &[0x7F, 0x10, 0x78], None), unsolicited);
        assert_eq!(classify(None, &[0x62, 0xF4, 0x0D, 0x40], None), unsolicited);
        assert_eq!(
            classify(
                Some(0x10),
                &[0x50, 0x03],
                Some(SessionSelection::NonDefault)
            ),
            ClientRx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: Some(SessionSelection::NonDefault),
            }
        );
    }

    /// #17 item 4, ISO 14229-1:2020 Table 29 — `P2*Server_max` is read in 10 ms units:
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
            Final::Malformed(RecordError::Overlong { declared: Some(9) })
        ));
    }
}
