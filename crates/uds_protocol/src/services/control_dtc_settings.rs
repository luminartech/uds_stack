//! `ControlDTCSetting` (0x85) service implementation
use crate::shared::SuppressablePositiveResponse;
use crate::{Decode, Encode, Error, Incomplete, NegativeResponseCode};
use automotive_wire_codec::{write_bytes, write_u8};

/// Controls whether the server should enable or disable DTC status-bit updates.
///
/// Used by [`ControlDtcSettingRequest`] to instruct the server.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DtcSettingType {
    /// ISO/SAE reserved (`0x00`, `0x03`-`0x3F` and `0x7F`, ISO 14229-1:2020 Table 128).
    ///
    /// Built only by [`DtcSettingType::try_from`]. A server that supports none answers it
    /// `subFunctionNotSupported` (Table 132).
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    IsoSaeReserved(u8),
    /// Re-enable DTC status-bit updates.
    On,
    /// Disable DTC status-bit updates.
    Off,
    /// Reserved for use by vehicle manufacturers (`0x40`-`0x5F`, ISO 14229-1:2020 Table
    /// 128).
    ///
    /// Construct through [`DtcSettingType::try_from`] so the raw byte is range-checked and
    /// can never collide with the SPRMIB bit.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    VehicleManufacturerSpecific(u8),
    /// Reserved for use by system suppliers (`0x60`-`0x7E`, ISO 14229-1:2020 Table 128).
    ///
    /// Construct through [`DtcSettingType::try_from`] so the raw byte is range-checked and
    /// can never collide with the SPRMIB bit.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    SystemSupplierSpecific(u8),
}

impl From<DtcSettingType> for u8 {
    fn from(value: DtcSettingType) -> Self {
        match value {
            DtcSettingType::On => 0x01,
            DtcSettingType::Off => 0x02,
            DtcSettingType::IsoSaeReserved(value)
            | DtcSettingType::VehicleManufacturerSpecific(value)
            | DtcSettingType::SystemSupplierSpecific(value) => value,
        }
    }
}

impl TryFrom<u8> for DtcSettingType {
    type Error = Error;

    /// ISO 14229-1:2020 Table 128 assigns every value of the 7-bit sub-function.
    ///
    /// # Errors
    /// Returns [`Error::InvalidDtcSetting`] for a byte with bit 7 set, which is the
    /// suppressPosRspMsgIndicationBit rather than part of the setting.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::On),
            0x02 => Ok(Self::Off),
            0x00 | 0x03..=0x3F | 0x7F => Ok(Self::IsoSaeReserved(value)),
            0x40..=0x5F => Ok(Self::VehicleManufacturerSpecific(value)),
            0x60..=0x7E => Ok(Self::SystemSupplierSpecific(value)),
            _ => Err(Error::InvalidDtcSetting(value)),
        }
    }
}

/// Request for the server to enable or disable DTC status-bit updates.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ControlDtcSettingRequest<'d> {
    /// Whether the server should suppress the positive response (SPRMIB).
    pub suppress_positive_response: bool,
    /// The requested DTC logging setting.
    pub setting: DtcSettingType,
    /// Optional `DTCSettingControlOptionRecord`, empty when absent.
    ///
    /// Marked `U` (user option) in ISO 14229-1:2020 Table 127. Table 129 describes it as
    /// vehicle-manufacturer specific data qualifying the request, for example a list of
    /// the DTCs to turn on or off, and Table 132 reserves
    /// [`NegativeResponseCode::RequestOutOfRange`] for a server that detects an error in
    /// it.
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub option_record: &'d [u8],
}

const CONTROL_DTC_SETTING_NEGATIVE_RESPONSE_CODES: [NegativeResponseCode; 4] = [
    NegativeResponseCode::SubFunctionNotSupported,
    NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
    NegativeResponseCode::ConditionsNotCorrect,
    NegativeResponseCode::RequestOutOfRange,
];

impl<'d> ControlDtcSettingRequest<'d> {
    /// Create a new `ControlDtcSettingRequest` with no `DTCSettingControlOptionRecord`.
    #[must_use]
    pub const fn new(suppress_positive_response: bool, setting: DtcSettingType) -> Self {
        Self {
            suppress_positive_response,
            setting,
            option_record: &[],
        }
    }

    /// Create a request carrying a `DTCSettingControlOptionRecord`.
    ///
    /// The record's contents are vehicle-manufacturer specific (ISO 14229-1:2020 Table
    /// 129).
    #[must_use]
    pub const fn new_with_option_record(
        suppress_positive_response: bool,
        setting: DtcSettingType,
        option_record: &'d [u8],
    ) -> Self {
        Self {
            suppress_positive_response,
            setting,
            option_record,
        }
    }

    /// Get the allowed [`NegativeResponseCode`] variants for this request.
    #[must_use]
    pub fn allowed_nack_codes() -> &'static [NegativeResponseCode] {
        &CONTROL_DTC_SETTING_NEGATIVE_RESPONSE_CODES
    }
}

impl Encode for ControlDtcSettingRequest<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let sub_function = SuppressablePositiveResponse::new(
            self.suppress_positive_response,
            self.setting,
        );
        let mut written = write_u8(writer, u8::from(sub_function))?;
        written += write_bytes(writer, self.option_record)?;
        Ok(written)
    }
}

impl<'a> Decode<'a> for ControlDtcSettingRequest<'a> {
    type Error = crate::Error;

    /// The sub-function byte is mandatory; everything after it is the optional
    /// `DTCSettingControlOptionRecord` (ISO 14229-1:2020 Table 127, `Cvt` = `U`), whose
    /// length is not on the wire: the record runs to the end of the message.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        let [sub_function, option_record @ ..] = buf else {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        };
        let sub_function =
            SuppressablePositiveResponse::<DtcSettingType>::try_from(*sub_function)?;
        Ok((
            Self {
                suppress_positive_response: sub_function.suppress_positive_response(),
                setting: sub_function.value(),
                option_record,
            },
            &[],
        ))
    }
}

/// Positive response to a `ControlDtcSettingRequest`
///
/// The ECU will respond with a `ControlDtcSettingResponse` if the request was successful.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ControlDtcSettingResponse {
    /// The DTC logging setting that was set in the request
    pub setting: DtcSettingType,
}

impl ControlDtcSettingResponse {
    /// Create a new `ControlDtcSettingResponse`.
    #[must_use]
    pub const fn new(setting: DtcSettingType) -> Self {
        Self { setting }
    }
}

impl Encode for ControlDtcSettingResponse {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        Ok(write_u8(writer, u8::from(self.setting))?)
    }
}

impl<'a> Decode<'a> for ControlDtcSettingResponse {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        let setting = DtcSettingType::try_from(buf[0])?;
        Ok((Self { setting }, &buf[1..]))
    }
}

#[cfg(test)]
mod request {
    use super::*;
    use crate::{
        Decode, Encode, NegativeResponseCode, test_util::assert_encode_size_agrees,
    };

    #[test]
    fn simple_request() {
        let req = ControlDtcSettingRequest::new(true, DtcSettingType::On);
        let mut buffer = [0u8; 4];
        let written = Encode::encode(
            &req,
            &mut automotive_wire_codec::SliceSink::new(&mut buffer),
        )
        .unwrap();
        assert_eq!(&buffer[..written], &[0x81]);
        assert_eq!(req.encoded_size().unwrap(), written);

        let (parsed, _) =
            <ControlDtcSettingRequest as Decode>::decode(&buffer[..written]).unwrap();
        assert_eq!(parsed.setting, DtcSettingType::On);
        assert!(parsed.suppress_positive_response);
        assert_encode_size_agrees(&req);
    }

    #[test]
    fn manufacturer_and_supplier_specific_setting_types_are_supported() {
        // ISO 14229-1:2020 Table 128 reserves 0x40-0x5F for vehicle-manufacturer use and
        // 0x60-0x7E for system-supplier use. Both decode, so a server sees the byte it
        // answers SubFunctionNotSupported to and a client can send a manufacturer-defined
        // setting.
        for (byte, expected) in [
            (0x40u8, DtcSettingType::VehicleManufacturerSpecific(0x40)),
            (0x5F, DtcSettingType::VehicleManufacturerSpecific(0x5F)),
            (0x60, DtcSettingType::SystemSupplierSpecific(0x60)),
            (0x7E, DtcSettingType::SystemSupplierSpecific(0x7E)),
        ] {
            let wire = [byte];
            let (req, _) = <ControlDtcSettingRequest as Decode>::decode(&wire).unwrap();
            assert_eq!(req.setting, expected, "for sub-function {byte:#04X}");

            let mut buf = [0u8; 4];
            let written =
                Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                    .unwrap();
            assert_eq!(&buf[..written], &[byte], "round trip for {byte:#04X}");
        }
    }

    #[test]
    fn reserved_setting_types_decode_as_reserved() {
        // Table 128 reserves 0x00, 0x03-0x3F and 0x7F. They classify rather than fail, as
        // every sibling sub-function's reserved values do; a server answers them 0x12
        // from its own lookup (Table 132).
        for byte in [0x00u8, 0x03, 0x3F, 0x7F] {
            let wire = [byte];
            let (req, _) = <ControlDtcSettingRequest as Decode>::decode(&wire).unwrap();
            assert!(
                matches!(req.setting, DtcSettingType::IsoSaeReserved(got) if got == byte),
                "{byte:#04X}"
            );
            assert_eq!(u8::from(req.setting), byte);
        }
    }

    #[test]
    fn a_dtc_setting_control_option_record_round_trips() {
        // Table 127 marks DTCSettingControlOptionRecord `U`, and Table 129 describes it as
        // carrying e.g. a list of DTCs to turn on or off. Table 132 reserves NRC 0x31 for
        // an error *in that record*, which a server can only detect if it is decoded.
        let wire = [0x85, 0x02, 0xAA, 0xBB, 0xCC];
        let (req, _) = crate::Request::decode(&wire).unwrap();
        let crate::Request::ControlDtcSetting(inner) = req else {
            panic!("expected a ControlDtcSetting request, got {req:?}");
        };
        assert_eq!(inner.setting, DtcSettingType::Off);
        assert_eq!(inner.option_record, &[0xAA, 0xBB, 0xCC]);

        let mut buf = [0u8; 8];
        let written = req.encode_to_slice(&mut buf).unwrap();
        assert_eq!(&buf[..written], &wire);
    }

    #[test]
    fn an_absent_option_record_is_an_empty_slice() {
        // Table 133's flow example is the bare two-byte form: an absent record encodes as
        // nothing, not as a zero byte.
        let (req, _) = crate::Request::decode(&[0x85, 0x02]).unwrap();
        let crate::Request::ControlDtcSetting(inner) = req else {
            panic!("expected a ControlDtcSetting request");
        };
        assert_eq!(inner.option_record, [0u8; 0]);

        let mut buf = [0u8; 8];
        let written = req.encode_to_slice(&mut buf).unwrap();
        assert_eq!(&buf[..written], &[0x85, 0x02]);
    }

    #[test]
    fn a_setting_byte_with_bit_7_carries_the_value() {
        // Bit 7 is the suppressPosRspMsgIndicationBit, not part of the setting, so a byte
        // with it set is no DTCSettingType and surfaces the offending byte.
        assert!(matches!(
            DtcSettingType::try_from(0x81),
            Err(Error::InvalidDtcSetting(0x81))
        ));
    }

    #[test]
    fn exposes_allowed_nack_codes() {
        assert_ne!(ControlDtcSettingRequest::allowed_nack_codes(), []);
        assert!(
            ControlDtcSettingRequest::allowed_nack_codes()
                .contains(&NegativeResponseCode::RequestOutOfRange)
        );
    }
}

#[cfg(test)]
mod response {
    use super::*;
    use crate::{Decode, Encode, test_util::assert_encode_size_agrees};

    #[test]
    fn simple_response() {
        let req = ControlDtcSettingResponse::new(DtcSettingType::On);
        let mut buffer = [0u8; 4];
        let written = Encode::encode(
            &req,
            &mut automotive_wire_codec::SliceSink::new(&mut buffer),
        )
        .unwrap();
        assert_eq!(&buffer[..written], &[0x01]);
        assert_eq!(req.encoded_size().unwrap(), written);

        let (parsed, _) =
            <ControlDtcSettingResponse as Decode>::decode(&buffer[..written]).unwrap();
        assert_eq!(parsed.setting, DtcSettingType::On);
        assert_encode_size_agrees(&req);
    }

    #[test]
    fn response_is_eq() {
        crate::test_util::assert_impl_eq::<ControlDtcSettingResponse>();
    }
}
