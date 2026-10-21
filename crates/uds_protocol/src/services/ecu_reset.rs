//! `ECUReset` (0x11) service implementation
use crate::shared::SuppressablePositiveResponse;
use crate::{Decode, Encode, Error, Incomplete, NegativeResponseCode};
use automotive_wire_codec::write_u8;

/// The `resetType` sub-function: which reset the server is to perform (ISO 14229-1:2020
/// Table 34).
///
/// Conversion from `u8` is fallible and returns an [`Error`] if bit 7, the suppress
/// positive response bit, is set.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetType {
    /// This value is reserved.
    ///
    /// Construct reserved values through [`ResetType::try_from`] so the raw byte is
    /// range-checked (`0x00..=0x7F`) and can never collide with the SPRMIB bit.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    IsoSaeReserved(u8),
    /// A "hard reset": simulates the power-on sequence after the server is disconnected
    /// from its power supply. The action is implementation specific; it may reinitialize
    /// both volatile and non-volatile memory to predetermined values.
    HardReset,
    /// Simulates an ignition off-and-on cycle, interrupting the switched power supply. The
    /// action is implementation specific; typically volatile memory is initialized and
    /// non-volatile memory preserved.
    KeyOffOnReset,
    /// A "soft reset": the server restarts its application program immediately. The action
    /// is implementation specific; typically configuration data, adaptive factors and other
    /// long-term adjustments survive.
    SoftReset,
    /// Enables the "rapid power shut down" function on a server that is battery powered
    /// rather than ignition powered: once the ignition is switched off the server
    /// transitions, directly or after a defined stand-by time, to sleep mode, from which
    /// the next ignition-on wakes it. If a response is required, the server sends it before
    /// shutting down. Applies only to a server with a stand-by mode.
    EnableRapidPowerShutDown,
    /// Disables the "rapid power shut down" function.
    DisableRapidPowerShutDown,
    /// Reserved for use by vehicle manufacturers.
    ///
    /// Construct through [`ResetType::try_from`] so the raw byte is range-checked
    /// (`0x40..=0x5F`) and can never collide with the SPRMIB bit.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    VehicleManufacturerSpecific(u8),
    /// Reserved for use by system suppliers.
    ///
    /// Construct through [`ResetType::try_from`] so the raw byte is range-checked
    /// (`0x60..=0x7E`) and can never collide with the SPRMIB bit.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    SystemSupplierSpecific(u8),
}

impl From<ResetType> for u8 {
    #[allow(clippy::match_same_arms)]
    fn from(value: ResetType) -> Self {
        match value {
            ResetType::IsoSaeReserved(val) => val,
            ResetType::HardReset => 0x01,
            ResetType::KeyOffOnReset => 0x02,
            ResetType::SoftReset => 0x03,
            ResetType::EnableRapidPowerShutDown => 0x04,
            ResetType::DisableRapidPowerShutDown => 0x05,
            ResetType::VehicleManufacturerSpecific(val) => val,
            ResetType::SystemSupplierSpecific(val) => val,
        }
    }
}

impl TryFrom<u8> for ResetType {
    type Error = Error;
    #[allow(clippy::match_same_arms)]
    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            0x00 => Ok(Self::IsoSaeReserved(0)),
            0x01 => Ok(Self::HardReset),
            0x02 => Ok(Self::KeyOffOnReset),
            0x03 => Ok(Self::SoftReset),
            0x04 => Ok(Self::EnableRapidPowerShutDown),
            0x05 => Ok(Self::DisableRapidPowerShutDown),
            0x06..=0x3F => Ok(Self::IsoSaeReserved(value)),
            0x40..=0x5F => Ok(Self::VehicleManufacturerSpecific(value)),
            0x60..=0x7E => Ok(Self::SystemSupplierSpecific(value)),
            0x7F => Ok(Self::IsoSaeReserved(value)),
            _ => Err(Error::InvalidEcuResetType(value)),
        }
    }
}

#[cfg(test)]
mod reset_type_tests {
    use super::*;
    /// Every byte classifies as ISO 14229-1:2020 Table 34 assigns it, or fails for bit 7.
    #[test]
    #[allow(clippy::match_same_arms)]
    fn reset_type_from_all_u8_values() {
        for i in 0..=u8::MAX {
            let reset_type: Result<ResetType, Error> = ResetType::try_from(i);
            match i {
                0x00 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::IsoSaeReserved(_)),
                )),
                0x01 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::HardReset),
                )),
                0x02 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::KeyOffOnReset),
                )),
                0x03 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::SoftReset),
                )),
                0x04 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::EnableRapidPowerShutDown),
                )),
                0x05 => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::DisableRapidPowerShutDown),
                )),
                0x06..=0x3F => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::IsoSaeReserved(_)),
                )),
                0x40..=0x5F => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::VehicleManufacturerSpecific(_)),
                )),
                0x60..=0x7E => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::SystemSupplierSpecific(_)),
                )),
                0x7F => assert!(matches!(
                    reset_type,
                    Ok::<ResetType, Error>(ResetType::IsoSaeReserved(_)),
                )),
                _ => assert!(matches!(
                    reset_type,
                    Err::<ResetType, Error>(Error::InvalidEcuResetType(_)),
                )),
            }
        }
    }

    #[test]
    fn reset_type_to_all_u8_values() {
        assert_eq!(u8::from(ResetType::IsoSaeReserved(0)), 0x00);
        assert_eq!(u8::from(ResetType::HardReset), 0x01);
        assert_eq!(u8::from(ResetType::KeyOffOnReset), 0x02);
        assert_eq!(u8::from(ResetType::SoftReset), 0x03);
        assert_eq!(u8::from(ResetType::EnableRapidPowerShutDown), 0x04);
        assert_eq!(u8::from(ResetType::DisableRapidPowerShutDown), 0x05);
    }
}

const ECU_RESET_NEGATIVE_RESPONSE_CODES: [NegativeResponseCode; 4] = [
    NegativeResponseCode::SubFunctionNotSupported,
    NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
    NegativeResponseCode::ConditionsNotCorrect,
    NegativeResponseCode::SecurityAccessDenied,
];

/// Request for the server to reset the ECU
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct EcuResetRequest {
    /// Whether the server should suppress a positive response (SPRMIB).
    pub suppress_positive_response: bool,
    /// The type of reset being requested.
    pub reset_type: ResetType,
}

impl EcuResetRequest {
    /// Create a new '`EcuResetRequest`'
    #[must_use]
    pub const fn new(suppress_positive_response: bool, reset_type: ResetType) -> Self {
        Self {
            suppress_positive_response,
            reset_type,
        }
    }

    /// Get the allowed [`NegativeResponseCode`] variants for this request
    #[must_use]
    pub fn allowed_nack_codes() -> &'static [NegativeResponseCode] {
        &ECU_RESET_NEGATIVE_RESPONSE_CODES
    }
}

impl Encode for EcuResetRequest {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        // Fuse the SPRMIB bit into the sub-function byte only at the wire boundary.
        let sub_function = SuppressablePositiveResponse::new(
            self.suppress_positive_response,
            self.reset_type,
        );
        Ok(write_u8(writer, u8::from(sub_function))?)
    }
}

impl<'a> Decode<'a> for EcuResetRequest {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        let sub_function = SuppressablePositiveResponse::<ResetType>::try_from(buf[0])?;
        Ok((
            Self {
                suppress_positive_response: sub_function.suppress_positive_response(),
                reset_type: sub_function.value(),
            },
            &buf[1..],
        ))
    }
}

/// Positive response to an `EcuResetRequest`
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct EcuResetResponse {
    /// The reset type echoed from the request.
    pub reset_type: ResetType,
    /// Minimum stand-by time the server will remain in the power-down sequence, at one
    /// second per count.
    ///
    /// `0x00`-`0xFE` are 0 to 254 seconds; `0xFF` indicates a failure or that the time is
    /// not available (ISO 14229-1:2020 Table 36).
    ///
    /// `None` means the byte is absent from the wire, which is the ordinary case: the
    /// parameter is marked `C` (conditional) in Table 35 and is present only when
    /// `reset_type` is [`ResetType::EnableRapidPowerShutDown`]. `None` is therefore
    /// distinct from `Some(0)`, which is a server reporting zero seconds.
    pub power_down_time: Option<u8>,
}

impl EcuResetResponse {
    /// Create a response that carries no `powerDownTime`, which is every reset type except
    /// [`ResetType::EnableRapidPowerShutDown`].
    #[must_use]
    pub const fn new(reset_type: ResetType) -> Self {
        Self {
            reset_type,
            power_down_time: None,
        }
    }

    /// Create a response that reports a `powerDownTime`, as
    /// [`ResetType::EnableRapidPowerShutDown`] requires.
    ///
    /// Pass `0xFF` to report a failure or that the time is not available.
    #[must_use]
    pub const fn new_with_power_down_time(
        reset_type: ResetType,
        power_down_time: u8,
    ) -> Self {
        Self {
            reset_type,
            power_down_time: Some(power_down_time),
        }
    }
}

impl Encode for EcuResetResponse {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let mut written = write_u8(writer, u8::from(self.reset_type))?;
        if let Some(power_down_time) = self.power_down_time {
            written += write_u8(writer, power_down_time)?;
        }
        Ok(written)
    }
}

impl<'a> Decode<'a> for EcuResetResponse {
    type Error = crate::Error;

    /// The `resetType` echo is mandatory; a second byte, if present, is the conditional
    /// `powerDownTime` (ISO 14229-1:2020 Table 35, `Cvt` = `C`).
    ///
    /// Presence is taken from the wire rather than inferred from `resetType`, so a response
    /// from a server that sends the byte outside `enableRapidPowerShutDown` still
    /// round-trips unchanged.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        let [reset_type, rest @ ..] = buf else {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        };
        let reset_type = ResetType::try_from(*reset_type)?;
        let (power_down_time, rest) = match rest {
            [] => (None, rest),
            [time, tail @ ..] => (Some(*time), tail),
        };
        Ok((
            Self {
                reset_type,
                power_down_time,
            },
            rest,
        ))
    }
}

#[cfg(test)]
mod request {
    use super::*;
    use crate::{Decode, Encode, test_util::assert_encode_size_agrees};

    #[test]
    fn ecu_reset_request() {
        let bytes: [u8; 2] = [0x81, 0x00];
        let req = EcuResetRequest::new(true, ResetType::HardReset);
        let mut buffer = [0u8; 4];
        let written = Encode::encode(
            &req,
            &mut automotive_wire_codec::SliceSink::new(&mut buffer),
        )
        .unwrap();
        // The request is one byte: the second byte of `bytes` is the next frame's, and the
        // decoder leaves it alone.
        assert_eq!(&buffer[..written], &bytes[..1]);
        let (result, _) = <EcuResetRequest as Decode>::decode(&bytes).unwrap();
        assert_eq!(result, req);

        assert_eq!(written, 1);
        assert_eq!(written, req.encoded_size().unwrap());
        assert_encode_size_agrees(&req);
    }
}

#[cfg(test)]
mod response {
    use super::*;
    use crate::{Decode, Encode, test_util::assert_encode_size_agrees};

    #[test]
    fn ecu_reset_response() {
        let bytes: [u8; 2] = [0x04, 0x20];
        let resp = EcuResetResponse::new_with_power_down_time(
            ResetType::EnableRapidPowerShutDown,
            0x20,
        );
        let mut buffer = [0u8; 4];
        let written = Encode::encode(
            &resp,
            &mut automotive_wire_codec::SliceSink::new(&mut buffer),
        )
        .unwrap();
        let (result, _) = <EcuResetResponse as Decode>::decode(&bytes).unwrap();
        assert_eq!(result, resp);

        // The encoded bytes themselves, not just their count, so a swap of the two written
        // bytes fails.
        assert_eq!(&buffer[..written], &bytes);
        assert_eq!(written, 2);
        assert_eq!(written, resp.encoded_size().unwrap());
        assert_encode_size_agrees(&resp);
    }

    #[test]
    fn a_reset_type_without_a_power_down_time_encodes_one_byte() {
        // ISO 14229-1:2020 Table 35 marks powerDownTime `C`, present only when the
        // sub-function is enableRapidPowerShutDown (0x04). Table 39's positive-response
        // flow example for hardReset is two bytes on the wire: `51 01`.
        let resp = EcuResetResponse::new(ResetType::HardReset);
        assert_eq!(resp.power_down_time, None);

        let mut buf = [0u8; 4];
        let written =
            Encode::encode(&resp, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &[0x01]);
        assert_encode_size_agrees(&resp);
    }

    #[test]
    fn a_response_without_a_power_down_time_round_trips_unchanged() {
        // Decoding and re-encoding does not invent a powerDownTime byte, so a proxy that
        // decodes and re-encodes ECU traffic passes every positive response unchanged.
        for wire in [[0x51, 0x01].as_slice(), [0x51, 0x04, 0x20].as_slice()] {
            let (resp, _) = crate::Response::decode(wire).unwrap();
            let mut buf = [0u8; 8];
            let written = resp.encode_to_slice(&mut buf).unwrap();
            assert_eq!(&buf[..written], wire, "round trip failed for {wire:02X?}");
        }
    }

    #[test]
    fn an_absent_power_down_time_is_distinguishable_from_a_reported_zero() {
        // 0x00 means "0 seconds" per Table 36; 0xFF is the failure/not-available sentinel.
        // Conflating absent with 0 loses that distinction.
        let (absent, _) = <EcuResetResponse as Decode>::decode(&[0x01]).unwrap();
        let (zero, _) = <EcuResetResponse as Decode>::decode(&[0x01, 0x00]).unwrap();
        assert_eq!(absent.power_down_time, None);
        assert_eq!(zero.power_down_time, Some(0));
        assert_ne!(absent, zero);
    }
}
