use crate::Error;

const LOW_NIBBLE_MASK: u8 = 0b0000_1111;
const HIGH_NIBBLE_MASK: u8 = 0b1111_0000;

/// Largest value that fits in a single nibble.
const NIBBLE_MAX: u8 = 0x0F;

/// Address and length format identifier
const MEMORY_SIZE_NIBBLE_MASK: u8 = HIGH_NIBBLE_MASK;
const MEMORY_ADDRESS_NIBBLE_MASK: u8 = LOW_NIBBLE_MASK;

/// Widest `memorySize` the `addressAndLengthFormatIdentifier` may declare, in bytes: the
/// eight a `u64` holds.
///
/// ISO 14229-1:2020 Annex H Table H.1 is informative and marks only a zero nibble "not
/// applicable". A wider declaration is refused by this crate, not by the standard.
pub(crate) const MAX_MEMORY_SIZE_LENGTH: u8 = 8;

/// Widest `memoryAddress` the `addressAndLengthFormatIdentifier` may declare, in bytes, for
/// the reason [`MAX_MEMORY_SIZE_LENGTH`] is.
pub(crate) const MAX_MEMORY_ADDRESS_LENGTH: u8 = 8;

/// Length format identifier
const BLOCK_LENGTH_NIBBLE_MASK: u8 = HIGH_NIBBLE_MASK;

/// Data format identifier
const COMPRESSION_NIBBLE_MASK: u8 = HIGH_NIBBLE_MASK;
const ENCRYPTION_NIBBLE_MASK: u8 = LOW_NIBBLE_MASK;

/// How many bytes the `memoryAddress` and `memorySize` parameters occupy on the wire.
///
/// The `addressAndLengthFormatIdentifier` byte of
/// [`RequestDownloadRequest`](crate::RequestDownloadRequest) and
/// [`RequestUploadRequest`](crate::RequestUploadRequest), which share one message layout.
/// The high nibble is the `memorySize` width and the low nibble the `memoryAddress` width,
/// in bytes, so `0x44` declares four bytes each, and `0x12` one byte of size and two of
/// address.
///
/// ISO 14229-1:2020 Table 441 makes this a client choice rather than a function of the
/// values, and clause 11.3.1 allows a fixed identifier with the unused bytes padded with
/// `0x00`. Pass one to
/// [`RequestDownloadRequest::new_with_alfid`](crate::RequestDownloadRequest::new_with_alfid);
/// [`new`](crate::RequestDownloadRequest::new) derives the narrowest one. A bootloader that
/// mandates a particular value states it as a byte, so build one the same way:
///
/// ```
/// use uds_protocol::AddressAndLengthFormatIdentifier as Alfid;
///
/// let alfid = Alfid::try_from(0x44).expect("four bytes each");
/// assert_eq!(alfid.memory_size_length(), 4);
/// assert_eq!(alfid.memory_address_length(), 4);
/// assert_eq!(u8::from(alfid), 0x44);
///
/// // Annex H Table H.1 marks a zero nibble "not applicable", and this crate holds at
/// // most eight bytes of each.
/// assert!(Alfid::try_from(0x40).is_err());
/// assert!(Alfid::try_from(0x88).is_ok());
/// assert!(Alfid::try_from(0x98).is_err());
/// ```
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressAndLengthFormatIdentifier {
    /// Width of `memorySize` in bytes: 1 to 8. Private so it cannot leave that range.
    memory_size_length: u8,
    /// Width of `memoryAddress` in bytes: 1 to 8. Private for the same reason.
    memory_address_length: u8,
}

impl AddressAndLengthFormatIdentifier {
    /// Build one from its two widths, in bytes.
    ///
    /// Arguments are in **wire order**: `memory_size_length` is the high nibble,
    /// `memory_address_length` the low nibble. Both are `u8`, so the compiler cannot catch
    /// a transposition; if you have the byte already, prefer `try_from`.
    ///
    /// # Errors
    /// Returns [`Error::InvalidAddressAndLengthFormatIdentifier`] carrying the byte the
    /// widths would have formed if either is outside 1 to 8.
    pub const fn new(
        memory_size_length: u8,
        memory_address_length: u8,
    ) -> Result<Self, Error> {
        if !matches!(memory_size_length, 1..=MAX_MEMORY_SIZE_LENGTH)
            || !matches!(memory_address_length, 1..=MAX_MEMORY_ADDRESS_LENGTH)
        {
            // Report the byte, not the nibble, so the message matches what a tester
            // configures and what `try_from` would have been given. Nibbles wider than 4
            // bits are masked so the reported byte is a packing of the two.
            return Err(Error::InvalidAddressAndLengthFormatIdentifier(
                ((memory_size_length & LOW_NIBBLE_MASK) << 4)
                    | (memory_address_length & LOW_NIBBLE_MASK),
            ));
        }
        Ok(Self {
            memory_size_length,
            memory_address_length,
        })
    }

    /// The narrowest widths that hold `memory_address` and `memory_size`, at least one
    /// byte each.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a u64 is at most eight bytes"
    )]
    pub(crate) const fn narrowest(memory_address: u64, memory_size: u64) -> Self {
        const fn width(value: u64) -> u8 {
            let bytes = (u64::BITS - value.leading_zeros()).div_ceil(8) as u8;
            if bytes == 0 { 1 } else { bytes }
        }
        Self {
            memory_size_length: width(memory_size),
            memory_address_length: width(memory_address),
        }
    }

    /// Width of the `memorySize` parameter in bytes, 1 to 8. The high nibble.
    #[must_use]
    pub const fn memory_size_length(self) -> u8 {
        self.memory_size_length
    }

    /// Width of the `memoryAddress` parameter in bytes, 1 to 8. The low nibble.
    #[must_use]
    pub const fn memory_address_length(self) -> u8 {
        self.memory_address_length
    }
}

impl PartialEq<u8> for AddressAndLengthFormatIdentifier {
    /// Wire equality: compares the packed byte, so `alfid == 0x44` reads as it does in the
    /// spec.
    fn eq(&self, other: &u8) -> bool {
        (self.memory_size_length << 4) | self.memory_address_length == *other
    }
}

impl TryFrom<u8> for AddressAndLengthFormatIdentifier {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self, Error> {
        // High nibble: bytes used for the memorySize parameter; low nibble: for the
        // memoryAddress parameter.
        let memory_size_length = (value & MEMORY_SIZE_NIBBLE_MASK) >> 4;
        let memory_address_length = value & MEMORY_ADDRESS_NIBBLE_MASK;

        if !matches!(memory_size_length, 1..=MAX_MEMORY_SIZE_LENGTH) {
            return Err(Error::InvalidAddressAndLengthFormatIdentifier(value));
        }
        if !matches!(memory_address_length, 1..=MAX_MEMORY_ADDRESS_LENGTH) {
            return Err(Error::InvalidAddressAndLengthFormatIdentifier(value));
        }
        Ok(Self {
            memory_size_length,
            memory_address_length,
        })
    }
}

impl From<AddressAndLengthFormatIdentifier> for u8 {
    fn from(alfid: AddressAndLengthFormatIdentifier) -> u8 {
        (alfid.memory_size_length << 4) | alfid.memory_address_length
    }
}

/// The leading byte of a [`RequestDownloadResponse`](crate::RequestDownloadResponse) or
/// [`RequestUploadResponse`](crate::RequestUploadResponse), which share one message layout.
///
/// The format mirrors [`AddressAndLengthFormatIdentifier`]: a byte whose high nibble gives
/// the byte length of `max_number_of_block_length`, i.e. `0x20` means that field is 2 bytes
/// long. Derived from the slice length when encoding, so it is not part of either
/// response's public surface.
///
/// ISO 14229-1:2020 Tables 443 and 448 reserve bits 3 to 0, "to be set to '0'": they are
/// ignored on decode and encoded as zero. Tables 442 and 447 make the first
/// `maxNumberOfBlockLength` byte mandatory, so a zero high nibble does not convert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LengthFormatIdentifier {
    pub max_number_of_block_length: u8,
}

impl TryFrom<u8> for LengthFormatIdentifier {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Error> {
        let max_number_of_block_length = (value & BLOCK_LENGTH_NIBBLE_MASK) >> 4;
        if max_number_of_block_length == 0 {
            return Err(Error::IncorrectMessageLengthOrInvalidFormat);
        }
        Ok(Self {
            max_number_of_block_length,
        })
    }
}
impl From<LengthFormatIdentifier> for u8 {
    fn from(length_format_identifier: LengthFormatIdentifier) -> u8 {
        length_format_identifier.max_number_of_block_length << 4
    }
}

/// The compression method (high nibble) and encryption method (low nibble) a client asks
/// the server to use for a transfer. `0x00` for both, [`DataFormatIdentifier::NONE`],
/// means neither; every other value is vehicle-manufacturer specific.
///
/// Supplied to [`RequestDownloadRequest::new`](crate::RequestDownloadRequest::new) and
/// [`RequestUploadRequest::new`](crate::RequestUploadRequest::new), read back with
/// [`RequestDownloadRequest::data_format_identifier`](crate::RequestDownloadRequest::data_format_identifier),
/// and carried by the `AddFile`, `ReplaceFile`, `ReadFile` and `ResumeFile` variants of
/// [`RequestFileTransferRequest`](crate::RequestFileTransferRequest). Serializes as the
/// single wire byte it occupies, which has no state for a nibble wider than `0x0F`.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(from = "u8", into = "u8"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataFormatIdentifier {
    // low nibble
    encryption_method: u8,
    // high nibble
    compression_method: u8,
}

impl DataFormatIdentifier {
    /// No compression and no encryption: the `0x00` byte, and the common case.
    pub const NONE: Self = Self {
        compression_method: 0,
        encryption_method: 0,
    };

    /// Build a `DataFormatIdentifier` from its compression and encryption method nibbles.
    ///
    /// Arguments are in **wire order**: compression is the high nibble, encryption the low
    /// nibble. `0x00` for both means no compression and no encryption; prefer
    /// [`DataFormatIdentifier::NONE`] for that. Both parameters are `u8`, so the compiler
    /// cannot catch a transposition; for a byte you already have, use
    /// `DataFormatIdentifier::from(byte)` instead.
    ///
    /// # Errors
    /// Returns [`Error::InvalidEncryptionCompressionMethod`] if either value does not fit
    /// in a nibble (is greater than `0x0F`).
    pub const fn new(compression_method: u8, encryption_method: u8) -> Result<Self, Error> {
        if compression_method > NIBBLE_MAX {
            return Err(Error::InvalidEncryptionCompressionMethod(
                compression_method,
            ));
        }
        if encryption_method > NIBBLE_MAX {
            return Err(Error::InvalidEncryptionCompressionMethod(encryption_method));
        }
        Ok(Self {
            encryption_method,
            compression_method,
        })
    }

    /// The compression method nibble (the high nibble on the wire).
    ///
    /// `0x00` means no compression. Other values are vehicle-manufacturer specific.
    #[must_use]
    pub const fn compression_method(&self) -> u8 {
        self.compression_method
    }

    /// The single byte this identifier occupies on the wire.
    ///
    /// `const`, unlike `u8::from(identifier)`.
    #[must_use]
    pub const fn value(&self) -> u8 {
        self.encryption_method | (self.compression_method << 4)
    }

    /// The encryption method nibble (the low nibble on the wire).
    ///
    /// `0x00` means no encryption. Other values are vehicle-manufacturer specific.
    #[must_use]
    pub const fn encryption_method(&self) -> u8 {
        self.encryption_method
    }
}
impl From<u8> for DataFormatIdentifier {
    fn from(value: u8) -> Self {
        let encryption_method = value & ENCRYPTION_NIBBLE_MASK;
        let compression_method = (value & COMPRESSION_NIBBLE_MASK) >> 4;

        Self {
            encryption_method,
            compression_method,
        }
    }
}
impl From<DataFormatIdentifier> for u8 {
    fn from(data_format_identifier: DataFormatIdentifier) -> u8 {
        data_format_identifier.encryption_method
            | (data_format_identifier.compression_method << 4)
    }
}

impl PartialEq<u8> for DataFormatIdentifier {
    fn eq(&self, other: &u8) -> bool {
        let other_data_format_identifier = DataFormatIdentifier::from(*other);
        self == &other_data_format_identifier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_format_identifier() {
        let memory_format_identifier =
            AddressAndLengthFormatIdentifier::try_from(0x23).unwrap();
        assert_eq!(memory_format_identifier.memory_size_length, 2);
        assert_eq!(memory_format_identifier.memory_address_length, 3);

        assert_eq!(u8::from(memory_format_identifier), 0x23);
    }

    #[test]
    fn failed_memory_format_identifier() {
        // Tables 444 and 449 both put an invalid addressAndLengthFormatIdentifier on NRC
        // 0x31 requestOutOfRange, not 0x13.
        let memory_format_identifier = AddressAndLengthFormatIdentifier::try_from(0x00);
        assert!(matches!(
            memory_format_identifier,
            Err(Error::InvalidAddressAndLengthFormatIdentifier(0x00))
        ));
        assert_eq!(
            Error::InvalidAddressAndLengthFormatIdentifier(0x00).negative_response_code(),
            Some(crate::NegativeResponseCode::RequestOutOfRange)
        );
    }

    #[test]
    fn every_alfid_of_one_to_eight_bytes_is_accepted() {
        // Annex H Table H.1 marks only a zero nibble "not applicable"; a u64 carries up to
        // eight bytes of address and of size.
        for size_len in 1..=8u8 {
            for addr_len in 1..=8u8 {
                let byte = (size_len << 4) | addr_len;
                let mfi =
                    AddressAndLengthFormatIdentifier::try_from(byte).unwrap_or_else(|e| {
                        panic!("{byte:#04X} declares 1 to 8 bytes, got {e:?}")
                    });
                assert_eq!(mfi.memory_size_length, size_len, "for {byte:#04X}");
                assert_eq!(mfi.memory_address_length, addr_len, "for {byte:#04X}");
                assert_eq!(u8::from(mfi), byte, "round trip for {byte:#04X}");
            }
        }
    }

    #[test]
    fn a_zero_or_wider_than_eight_nibble_is_rejected() {
        for byte in [0x00, 0x01, 0x10, 0x08, 0x80, 0x19, 0x91, 0x99, 0xF8, 0x8F] {
            assert!(
                AddressAndLengthFormatIdentifier::try_from(byte).is_err(),
                "{byte:#04X} declares a zero width or one wider than eight, but was accepted"
            );
        }
    }

    #[test]
    fn length_format_identifier() {
        let length_format_identifier = LengthFormatIdentifier::try_from(0xF0).unwrap();
        assert_eq!(length_format_identifier.max_number_of_block_length, 15);

        assert_eq!(u8::from(length_format_identifier), 0xF0);
        for malformed in [0x00, 0x09] {
            assert!(matches!(
                LengthFormatIdentifier::try_from(malformed),
                Err(Error::IncorrectMessageLengthOrInvalidFormat)
            ));
        }
    }

    #[test]
    fn data_format_identifier() {
        let data_format_identifier = DataFormatIdentifier::from(0x23);
        assert_eq!(data_format_identifier.encryption_method, 3);
        assert_eq!(data_format_identifier.compression_method, 2);

        assert_eq!(u8::from(data_format_identifier), 0x23);

        let data_format_identifier = DataFormatIdentifier::new(0x0F, 0x0F);
        assert!(data_format_identifier.is_ok());

        let data_format_identifier = DataFormatIdentifier::new(0x1F, 0x0F);
        assert!(matches!(
            data_format_identifier,
            Err(Error::InvalidEncryptionCompressionMethod(0x1F))
        ));

        // Arguments are in wire order: compression is the high nibble.
        let dfi = DataFormatIdentifier::new(0x02, 0x01).unwrap();
        assert_eq!(dfi.compression_method(), 0x02);
        assert_eq!(dfi.encryption_method(), 0x01);
        assert_eq!(
            u8::from(dfi),
            0x21,
            "compression must land in the high nibble"
        );

        assert_eq!(u8::from(DataFormatIdentifier::NONE), 0x00);
        assert_eq!(DataFormatIdentifier::NONE, DataFormatIdentifier::from(0x00));
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn prop_data_format_identifier_roundtrip(byte in any::<u8>()) {
                let dfi = DataFormatIdentifier::from(byte);
                let back: u8 = dfi.into();
                prop_assert_eq!(byte, back);
            }

            #[test]
            fn prop_length_format_identifier_normalizes_the_reserved_nibble(byte in any::<u8>()) {
                // Over the full `u8` range: a byte converts exactly when its high nibble is
                // not zero, and re-encodes with the reserved low nibble cleared.
                let back = LengthFormatIdentifier::try_from(byte).map_or(0, u8::from);
                prop_assert_eq!(byte & 0xF0, back);
            }

            #[test]
            fn prop_memory_format_identifier_roundtrip(
                // The full accepted range, so an off-by-one in the range checks fails.
                size_len in 1u8..=MAX_MEMORY_SIZE_LENGTH,
                addr_len in 1u8..=MAX_MEMORY_ADDRESS_LENGTH,
            ) {
                let byte = (size_len << 4) | addr_len;
                let mfi = AddressAndLengthFormatIdentifier::try_from(byte).unwrap();
                let back: u8 = mfi.into();
                prop_assert_eq!(byte, back);
            }
        }
    }
}
