//! Routine Control (0x31) Service is used to perform functions on the ECU that may not be
//! covered by other services.
//!
//! It can also be used to check the ECU's health, erase memory, or other custom
//! manufacturer/supplier routines. However, some routines may have side effects or require
//! certain preconditions to be met.
use crate::shared::SuppressablePositiveResponse;
use crate::{Decode, Encode, Error, Incomplete, NegativeResponseCode};
use automotive_wire_codec::{write_bytes, write_u8, write_u16_be};

/// What type of routine control to perform for a [`RoutineControlRequest`].
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RoutineControlSubFunction {
    /// Routine will be started sometime between completion of the `StartRoutine` request
    /// and the completion of the 1st response message which indicates that the routine has
    /// already been performed, or is in progress
    ///
    /// It might be necessary to switch the server to a specific Diagnostic Session via
    /// [`crate::DiagnosticSessionControlRequest`] before starting the routine, or unlock
    /// the server using [`crate::SecurityAccessRequest`] before starting the routine.
    StartRoutine,

    /// The server routine shall be stopped in the server's memory sometime between the
    /// completion of the `StopRoutine` request and the completion of the 1st response
    /// message which indicates that the routine has already been stopped, or is in progress
    StopRoutine,

    /// Request results for the specified routineIdentifier
    RequestRoutineResults,

    /// A `routineControlType` ISO 14229-1:2020 Table 426 reserves: `0x00` or `0x04`-`0x7F`.
    ///
    /// Decoded rather than rejected because Figure 30 answers it
    /// `subFunctionNotSupported` (0x12) only after the routine identifier's checks
    /// (`requestOutOfRange`, 0x31, and `securityAccessDenied`, 0x33), which a server can
    /// make only on a request that decoded. Never has bit 7 set. Built by
    /// [`TryFrom<u8>`](RoutineControlSubFunction::try_from), as decoding is, so a client
    /// can name and encode one; a server answers it 0x12, and a positive response
    /// echoing one does not decode.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[cfg_attr(feature = "serde", serde(skip_deserializing))]
    #[non_exhaustive]
    IsoSaeReserved(u8),
}

impl From<RoutineControlSubFunction> for u8 {
    fn from(value: RoutineControlSubFunction) -> Self {
        match value {
            RoutineControlSubFunction::StartRoutine => 0x01,
            RoutineControlSubFunction::StopRoutine => 0x02,
            RoutineControlSubFunction::RequestRoutineResults => 0x03,
            RoutineControlSubFunction::IsoSaeReserved(value) => value,
        }
    }
}

impl TryFrom<u8> for RoutineControlSubFunction {
    type Error = Error;

    /// ISO 14229-1:2020 Table 426 defines `0x01`-`0x03` and reserves the rest of
    /// `0x00`-`0x7F`, with no vehicle-manufacturer or system-supplier range; a reserved
    /// value is [`IsoSaeReserved`](Self::IsoSaeReserved).
    ///
    /// # Errors
    /// Returns [`Error::InvalidRoutineControlSubFunction`] for a value with bit 7 set: that
    /// bit is the suppressPosRspMsgIndicationBit, not part of the `routineControlType`.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(RoutineControlSubFunction::StartRoutine),
            0x02 => Ok(RoutineControlSubFunction::StopRoutine),
            0x03 => Ok(RoutineControlSubFunction::RequestRoutineResults),
            0x00 | 0x04..=0x7F => Ok(RoutineControlSubFunction::IsoSaeReserved(value)),
            _ => Err(Error::InvalidRoutineControlSubFunction(value)),
        }
    }
}

const ROUTINE_CONTROL_NEGATIVE_RESPONSE_CODES: [NegativeResponseCode; 7] = [
    NegativeResponseCode::SubFunctionNotSupported,
    NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
    NegativeResponseCode::ConditionsNotCorrect,
    NegativeResponseCode::RequestSequenceError,
    NegativeResponseCode::RequestOutOfRange,
    NegativeResponseCode::SecurityAccessDenied,
    NegativeResponseCode::GeneralProgrammingFailure,
];

/// Used by a client to execute a defined sequence of events and obtain any relevant
/// results.
///
/// The 2-byte big-endian routine identifier is decoded into a typed `u16`, followed by
/// optional routine input parameters in `option_record`.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RoutineControlRequest<'d> {
    /// Whether the server should suppress the positive response (SPRMIB).
    pub suppress_positive_response: bool,
    /// The routine control operation (start, stop, or request results).
    pub sub_function: RoutineControlSubFunction,
    /// The 16-bit routine identifier.
    pub routine_id: u16,
    /// Optional routine input parameters (may be empty).
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub option_record: &'d [u8],
}

impl<'d> RoutineControlRequest<'d> {
    /// Create a new `RoutineControlRequest`.
    #[must_use]
    pub const fn new(
        suppress_positive_response: bool,
        sub_function: RoutineControlSubFunction,
        routine_id: u16,
        option_record: &'d [u8],
    ) -> Self {
        Self {
            suppress_positive_response,
            sub_function,
            routine_id,
            option_record,
        }
    }

    /// Get the allowed [`NegativeResponseCode`] variants for this request.
    #[must_use]
    pub fn allowed_nack_codes() -> &'static [NegativeResponseCode] {
        &ROUTINE_CONTROL_NEGATIVE_RESPONSE_CODES
    }
}

impl Encode for RoutineControlRequest<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let sub_function = SuppressablePositiveResponse::new(
            self.suppress_positive_response,
            self.sub_function,
        );
        let mut written = write_u8(writer, u8::from(sub_function))?;
        written += write_u16_be(writer, self.routine_id)?;
        written += write_bytes(writer, self.option_record)?;
        Ok(written)
    }
}

impl<'a> Decode<'a> for RoutineControlRequest<'a> {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.len() < 3 {
            return Err(Error::InsufficientData(Incomplete {
                needed: 3,
                available: buf.len(),
            }));
        }
        let sub_function =
            SuppressablePositiveResponse::<RoutineControlSubFunction>::try_from(buf[0])?;
        let routine_id = u16::from_be_bytes([buf[1], buf[2]]);
        Ok((
            Self {
                suppress_positive_response: sub_function.suppress_positive_response(),
                sub_function: sub_function.value(),
                routine_id,
                option_record: &buf[3..],
            },
            &[],
        ))
    }
}

/// `RoutineControlResponse` is a variable-length response that can contain routine status.
///
/// The 2-byte big-endian routine identifier echo is decoded into a typed `u16`, followed
/// by optional routine status bytes in `status_record`.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RoutineControlResponse<'d> {
    /// The routine control operation echoed from the request (start, stop, or request
    /// results).
    pub sub_function: RoutineControlSubFunction,
    /// The 16-bit routine identifier echoed from the request.
    pub routine_id: u16,
    /// Everything the server sent after the routine identifier — bytes `#5` onward of
    /// ISO 14229-1:2020 Table 428. May be empty.
    ///
    /// Note that this is **not** only the `routineStatusRecord`. Table 428 places an
    /// optional `routineInfo` byte at `#5`, immediately before the status record, and
    /// whether it is present is vehicle-manufacturer defined — nothing on the wire
    /// distinguishes the two layouts, so this crate cannot split them for you. If the
    /// server you are talking to sends `routineInfo`, it is the first byte here and the
    /// status record starts at index 1.
    ///
    /// `routineInfo` itself is vehicle-manufacturer specific (Table 429); it exists so
    /// generic test equipment can handle all implemented routines uniformly.
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub status_record: &'d [u8],
}

impl<'d> RoutineControlResponse<'d> {
    /// Create a new `RoutineControlResponse`.
    #[must_use]
    pub const fn new(
        sub_function: RoutineControlSubFunction,
        routine_id: u16,
        status_record: &'d [u8],
    ) -> Self {
        Self {
            sub_function,
            routine_id,
            status_record,
        }
    }
}

impl Encode for RoutineControlResponse<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let mut written = write_u8(writer, u8::from(self.sub_function))?;
        written += write_u16_be(writer, self.routine_id)?;
        written += write_bytes(writer, self.status_record)?;
        Ok(written)
    }
}

impl<'a> Decode<'a> for RoutineControlResponse<'a> {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.len() < 3 {
            return Err(Error::InsufficientData(Incomplete {
                needed: 3,
                available: buf.len(),
            }));
        }
        // Plain try_from (no SPRMIB mask): a set 0x80 bit on a response is malformed, and
        // so is a reserved routineControlType, which Figure 30 answers 0x12.
        let sub_function = match RoutineControlSubFunction::try_from(buf[0])? {
            RoutineControlSubFunction::IsoSaeReserved(value) => {
                return Err(Error::InvalidRoutineControlSubFunction(value));
            }
            defined => defined,
        };
        let routine_id = u16::from_be_bytes([buf[1], buf[2]]);
        Ok((
            Self {
                sub_function,
                routine_id,
                status_record: &buf[3..],
            },
            &[],
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_util::{assert_encode_size_agrees, assert_impl_eq};
    use crate::{Decode, NegativeResponseCode};

    #[test]
    fn derive_contract() {
        assert_impl_eq::<RoutineControlRequest<'_>>();
        assert_impl_eq::<RoutineControlResponse<'_>>();

        #[cfg(feature = "serde")]
        {
            use crate::test_util::assert_impl_serde;
            assert_impl_serde::<RoutineControlRequest<'_>>();
            assert_impl_serde::<RoutineControlResponse<'_>>();
        }
    }

    #[test]
    fn status_record_covers_routine_info_as_well() {
        // ISO 14229-1:2020 Table 428 puts an optional `routineInfo` byte at #5, before the
        // routineStatusRecord, with presence left to the vehicle manufacturer. Nothing on
        // the wire distinguishes "routineInfo + status" from "status only", so
        // `status_record` deliberately spans both and the field docs say so. Pinned here
        // because a future reader might otherwise "fix" it into a wrong split.
        let wire = [0x71, 0x01, 0xF0, 0x0F, 0xAA, 0x32];
        let (resp, _) = crate::Response::decode(&wire).unwrap();
        let crate::Response::RoutineControl(inner) = resp else {
            panic!("expected a RoutineControl response, got {resp:?}");
        };
        assert_eq!(inner.routine_id, 0xF00F);
        assert_eq!(
            inner.status_record,
            &[0xAA, 0x32],
            "routineInfo must stay in the slice rather than being silently dropped"
        );

        let mut buf = [0u8; 8];
        let written = resp.encode_to_slice(&mut buf).unwrap();
        assert_eq!(&buf[..written], &wire);
    }

    #[test]
    fn a_reserved_sub_function_decodes_so_the_routine_checks_can_come_first() {
        // ISO 14229-1:2020 Table 426 defines only 0x01-0x03; 0x00 and 0x04-0x7F are
        // ISOSAEReserved. Figure 30 answers one 0x12, but only after the routine
        // identifier's 0x31 and 0x33 checks, so the request has to decode: rejecting it
        // here left a server no choice but to answer before those checks. It re-encodes
        // to the byte it came from, with the suppress bit kept apart.
        for byte in [0x00u8, 0x04, 0x10, 0x7F] {
            let request = [byte | 0x80, 0xF0, 0x0F];
            let (decoded, rest) = <RoutineControlRequest as Decode>::decode(&request)
                .expect("a reserved routineControlType decodes");
            assert_eq!(rest, [0_u8; 0]);
            assert_eq!(
                decoded.sub_function,
                RoutineControlSubFunction::IsoSaeReserved(byte)
            );
            assert!(decoded.suppress_positive_response);
            assert_eq!(u8::from(decoded.sub_function), byte);
        }
    }

    #[test]
    fn a_sub_function_byte_with_bit_7_is_answered_with_sub_function_not_supported() {
        // Bit 7 is the suppressPosRspMsgIndicationBit, so it never names a
        // routineControlType; `try_from` on a byte carrying it is the error, and the error
        // maps to 0x12 as Table 430 requires.
        let err =
            RoutineControlSubFunction::try_from(0x81).expect_err("bit 7 is not a type");
        assert!(matches!(err, Error::InvalidRoutineControlSubFunction(0x81)));
        assert_eq!(
            err.negative_response_code(),
            Some(NegativeResponseCode::SubFunctionNotSupported)
        );
    }

    #[test]
    fn a_response_echoing_a_reserved_routine_control_type_is_rejected() {
        // ISO 14229-1:2020 Table 428 echoes the request's routineControlType, and Figure
        // 30 answers a reserved one 0x12, so a positive response carrying one is
        // malformed even though the request that named it decodes.
        for byte in [0x00u8, 0x04, 0x7F] {
            let err = <RoutineControlResponse as Decode>::decode(&[byte, 0x02, 0x01])
                .expect_err("a reserved routineControlType is not a response");
            assert!(
                matches!(err, Error::InvalidRoutineControlSubFunction(got) if got == byte),
                "{byte:#04X}: {err:?}"
            );
        }
    }

    #[test]
    fn rc_request_round_trips_with_suppress() {
        let req = RoutineControlRequest::new(
            true,
            RoutineControlSubFunction::StartRoutine,
            0xFF00,
            &[0xAA],
        );
        let mut buf = [0u8; 8];
        let n = Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
            .unwrap();
        assert_eq!(&buf[..n], &[0x81, 0xFF, 0x00, 0xAA]); // 0x81 = StartRoutine | SPRMIB
        let (d, rest) = <RoutineControlRequest as Decode>::decode(&buf[..n]).unwrap();
        assert_eq!(rest, [0u8; 0]);
        assert!(d.suppress_positive_response);
        assert_eq!(d.sub_function, RoutineControlSubFunction::StartRoutine);
        assert_eq!(d.routine_id, 0xFF00);
        assert_eq!(d.option_record, &[0xAA]);
        assert_encode_size_agrees(&req);
    }

    #[test]
    fn rc_request_rejects_short_buffer() {
        assert!(<RoutineControlRequest as Decode>::decode(&[0x01, 0xFF]).is_err());
    }

    #[test]
    fn rc_response_round_trips_and_rejects_sprmib_bit() {
        let resp = RoutineControlResponse::new(
            RoutineControlSubFunction::StartRoutine,
            0xFF00,
            &[0x10],
        );
        let mut buf = [0u8; 8];
        let n = Encode::encode(&resp, &mut automotive_wire_codec::SliceSink::new(&mut buf))
            .unwrap();
        assert_eq!(&buf[..n], &[0x01, 0xFF, 0x00, 0x10]);
        let (d, _) = <RoutineControlResponse as Decode>::decode(&buf[..n]).unwrap();
        assert_eq!(d.sub_function, RoutineControlSubFunction::StartRoutine);
        assert_eq!(d.routine_id, 0xFF00);
        // A response with the SPRMIB bit set (0x81) is malformed and rejected.
        assert!(<RoutineControlResponse as Decode>::decode(&[0x81, 0xFF, 0x00]).is_err());
        assert_encode_size_agrees(&resp);
    }

    #[test]
    fn exposes_allowed_nack_codes() {
        assert_ne!(RoutineControlRequest::allowed_nack_codes(), []);
        assert!(
            RoutineControlRequest::allowed_nack_codes()
                .contains(&NegativeResponseCode::SecurityAccessDenied)
        );
    }
}
