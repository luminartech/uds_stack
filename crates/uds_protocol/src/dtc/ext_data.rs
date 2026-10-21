use crate::{Decode, Encode, Error, Incomplete};
use automotive_wire_codec::write_u8;

/// Which stored `DTCExtDataRecord` a `ReadDTCInformation` request asks for.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(from = "u8", into = "u8"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DtcExtDataRecordNumber {
    /// ISO/SAE reserved record numbers (`0x00`, `0xF0-0xFD`).
    #[non_exhaustive]
    IsoSaeReserved(u8),

    /// Vehicle manufacturer-specific stored `DTCExtDataRecord`s
    ///
    /// 0x01-0x8F
    #[non_exhaustive]
    VehicleManufacturerSpecific(u8),

    /// A regulated-emissions OBD extended data record, numbered as SAE J1979-DA defines.
    ///
    /// 0x90-0x9F
    #[non_exhaustive]
    RegulatedEmissionsObdDtcExtDataRecords(u8),

    /// Regulated stored `DTCExtendedDataRecord`s.
    ///
    /// 0xA0-0xEF
    #[non_exhaustive]
    RegulatedDtcExtDataRecords(u8),

    /// Every regulated-emissions OBD extended data record the server stores.
    AllRegulatedEmissionsObdDtcExtDataRecords,

    /// Every extended data record the server stores.
    AllDtcExtDataRecords,
}

impl DtcExtDataRecordNumber {
    /// Create a new `DtcExtDataRecordNumber` from a raw byte, mapping it to the correct
    /// variant.
    #[must_use]
    pub const fn new(value: u8) -> Self {
        match value {
            0x00 | 0xF0..=0xFD => Self::IsoSaeReserved(value),
            0x01..=0x8F => Self::VehicleManufacturerSpecific(value),
            0x90..=0x9F => Self::RegulatedEmissionsObdDtcExtDataRecords(value),
            0xA0..=0xEF => Self::RegulatedDtcExtDataRecords(value),
            0xFE => Self::AllRegulatedEmissionsObdDtcExtDataRecords,
            0xFF => Self::AllDtcExtDataRecords,
        }
    }

    /// Return the raw `u8` value of this record number.
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub const fn value(&self) -> u8 {
        match self {
            Self::IsoSaeReserved(value) => *value,
            Self::VehicleManufacturerSpecific(value) => *value,
            Self::RegulatedEmissionsObdDtcExtDataRecords(value) => *value,
            Self::RegulatedDtcExtDataRecords(value) => *value,
            Self::AllRegulatedEmissionsObdDtcExtDataRecords => 0xFE,
            Self::AllDtcExtDataRecords => 0xFF,
        }
    }
}

impl From<u8> for DtcExtDataRecordNumber {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl From<DtcExtDataRecordNumber> for u8 {
    fn from(value: DtcExtDataRecordNumber) -> Self {
        value.value()
    }
}

impl PartialEq<u8> for DtcExtDataRecordNumber {
    fn eq(&self, other: &u8) -> bool {
        self.value() == *other
    }
}

impl Encode for DtcExtDataRecordNumber {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        Ok(write_u8(writer, self.value())?)
    }
}

impl<'a> Decode<'a> for DtcExtDataRecordNumber {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        Ok((Self::new(buf[0]), &buf[1..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_number() {
        let record_number = DtcExtDataRecordNumber::new(0x00);
        assert_eq!(record_number, DtcExtDataRecordNumber::IsoSaeReserved(0x00));
        assert_eq!(record_number.value(), 0x00);
    }

    #[test]
    fn encode_ext_data_record_number() {
        use crate::test_util::assert_encode_size_agrees;
        let n = DtcExtDataRecordNumber::new(0x90);
        let mut buf = [0u8; 4];
        let written =
            crate::Encode::encode(&n, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(written, 1);
        assert_eq!(buf[0], 0x90);
        assert_encode_size_agrees(&n);
    }
}
