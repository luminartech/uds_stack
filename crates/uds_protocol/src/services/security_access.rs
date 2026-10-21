//! `SecurityAccess` (0x27) service implementation
use crate::shared::SuppressablePositiveResponse;
use crate::{Decode, Encode, Error, Incomplete, NegativeResponseCode};
use automotive_wire_codec::{write_bytes, write_u8};

/// One vehicle-manufacturer security level: the `requestSeed`/`sendKey` pair of ISO
/// 14229-1:2020 Table 42, named by its `requestSeed` sub-function.
///
/// Holding the pair rather than a raw sub-function byte is what keeps
/// [`SecurityAccessType::RequestSeed`] and [`SecurityAccessType::SendKey`] apart: each
/// encodes its own half, so a key cannot be sent as a seed request. Serializes as the
/// `requestSeed` byte.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SecurityAccessLevel(u8);

impl SecurityAccessLevel {
    /// The level whose `requestSeed` sub-function is `request_seed`.
    ///
    /// # Errors
    /// Returns [`Error::InvalidSecurityAccessType`] unless `request_seed` is one of Table
    /// 42's vehicle-manufacturer `requestSeed` values: odd, `0x01` to `0x41`.
    pub const fn from_request_seed(request_seed: u8) -> Result<Self, Error> {
        if request_seed % 2 == 1 && request_seed <= LAST_REQUEST_SEED {
            Ok(Self(request_seed))
        } else {
            Err(Error::InvalidSecurityAccessType(request_seed))
        }
    }

    /// The `requestSeed` sub-function value.
    #[must_use]
    pub const fn request_seed(self) -> u8 {
        self.0
    }

    /// The `sendKey` sub-function value, [`Self::request_seed`] plus one.
    #[must_use]
    pub const fn send_key(self) -> u8 {
        // At most `LAST_REQUEST_SEED + 1`, so the sum cannot overflow.
        self.0.saturating_add(1)
    }
}

/// The highest vehicle-manufacturer `requestSeed` value in Table 42.
const LAST_REQUEST_SEED: u8 = 0x41;

impl TryFrom<u8> for SecurityAccessLevel {
    type Error = Error;

    /// # Errors
    /// Returns [`Error::InvalidSecurityAccessType`] where `value` is not a `requestSeed`;
    /// see [`SecurityAccessLevel::from_request_seed`].
    fn try_from(value: u8) -> Result<Self, Error> {
        Self::from_request_seed(value)
    }
}

impl From<SecurityAccessLevel> for u8 {
    fn from(level: SecurityAccessLevel) -> Self {
        level.request_seed()
    }
}

/// The `securityAccessType` sub-function: whether a seed is requested or a key sent, and
/// for which security level (ISO 14229-1:2020 Table 42).
///
/// Conversion from `u8` is fallible and returns an [`Error`] if bit 7, the suppress
/// positive response bit, is set.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(try_from = "u8", into = "u8"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAccessType {
    /// This value is reserved for future definition
    ///
    /// Construct through [`SecurityAccessType::try_from`] so the raw byte is range-checked
    /// and can never collide with the SPRMIB bit.
    #[non_exhaustive]
    IsoSaeReserved(u8),
    /// `requestSeed` for a vehicle-manufacturer security level; encodes
    /// [`SecurityAccessLevel::request_seed`].
    RequestSeed(SecurityAccessLevel),
    /// `sendKey` for a vehicle-manufacturer security level; encodes
    /// [`SecurityAccessLevel::send_key`].
    SendKey(SecurityAccessLevel),
    /// `requestSeed` for the ISO 26021-2 security levels, which guard the end-of-life
    /// deployment of on-board pyrotechnic devices (`0x5F`, Table 42).
    Iso26021_2Values,
    /// `sendKey` for the ISO 26021-2 security levels (`0x60`, Table 42).
    Iso26021_2SendKeyValues,
    /// A byte Table 42 sets aside for system suppliers.
    ///
    /// Construct through [`SecurityAccessType::try_from`] so the raw byte is range-checked
    /// and can never collide with the SPRMIB bit.
    #[non_exhaustive]
    SystemSupplierSpecific(u8),
}

impl From<SecurityAccessType> for u8 {
    #[allow(clippy::match_same_arms)]
    fn from(value: SecurityAccessType) -> Self {
        match value {
            SecurityAccessType::IsoSaeReserved(val) => val,
            SecurityAccessType::RequestSeed(level) => level.request_seed(),
            SecurityAccessType::SendKey(level) => level.send_key(),
            SecurityAccessType::Iso26021_2Values => 0x5F,
            SecurityAccessType::Iso26021_2SendKeyValues => 0x60,
            SecurityAccessType::SystemSupplierSpecific(val) => val,
        }
    }
}

impl TryFrom<u8> for SecurityAccessType {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            0x00 | 0x43..=0x5E | 0x7F => Ok(Self::IsoSaeReserved(value)),
            // Security requests alternate, with odd numbers being seed requests,
            // and even numbers being send key requests
            0x01..=0x42 => {
                // An odd `value` is the level's own `requestSeed`; an even one is the
                // `sendKey` of the level one below it, which is odd and at least 0x01.
                if value % 2 == 1 {
                    Ok(Self::RequestSeed(SecurityAccessLevel(value)))
                } else {
                    Ok(Self::SendKey(SecurityAccessLevel(value - 1)))
                }
            }
            0x5F => Ok(Self::Iso26021_2Values),
            0x60 => Ok(Self::Iso26021_2SendKeyValues),
            0x61..=0x7E => Ok(Self::SystemSupplierSpecific(value)),
            _ => Err(Error::InvalidSecurityAccessType(value)),
        }
    }
}

#[cfg(test)]
mod security_access_type_tests {
    use super::*;

    const REQUEST_SEED_VALUES: [u8; 33] = [
        0x01, 0x03, 0x05, 0x07, 0x09, 0x0B, 0x0D, 0x0F, 0x11, 0x13, 0x15, 0x17, 0x19, 0x1B,
        0x1D, 0x1F, 0x21, 0x23, 0x25, 0x27, 0x29, 0x2B, 0x2D, 0x2F, 0x31, 0x33, 0x35, 0x37,
        0x39, 0x3B, 0x3D, 0x3F, 0x41,
    ];
    const SEND_KEY_VALUES: [u8; 33] = [
        0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C, 0x0E, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1A, 0x1C,
        0x1E, 0x20, 0x22, 0x24, 0x26, 0x28, 0x2A, 0x2C, 0x2E, 0x30, 0x32, 0x34, 0x36, 0x38,
        0x3A, 0x3C, 0x3E, 0x40, 0x42,
    ];
    /// Every byte classifies as Table 42 assigns it, or fails for bit 7.
    #[test]
    fn security_access_type_from_all_u8_values() {
        assert_eq!(
            SecurityAccessType::try_from(0).unwrap(),
            SecurityAccessType::IsoSaeReserved(0)
        );
        for value in &REQUEST_SEED_VALUES {
            assert_eq!(
                SecurityAccessType::try_from(*value).unwrap(),
                SecurityAccessType::RequestSeed(SecurityAccessLevel(*value))
            );
        }
        for value in &SEND_KEY_VALUES {
            assert_eq!(
                SecurityAccessType::try_from(*value).unwrap(),
                SecurityAccessType::SendKey(SecurityAccessLevel(*value - 1))
            );
        }
        for i in 0x43..=0x5E {
            assert_eq!(
                SecurityAccessType::try_from(i).unwrap(),
                SecurityAccessType::IsoSaeReserved(i)
            );
        }
        assert_eq!(
            SecurityAccessType::try_from(0x5F).unwrap(),
            SecurityAccessType::Iso26021_2Values
        );
        assert_eq!(
            SecurityAccessType::try_from(0x60).unwrap(),
            SecurityAccessType::Iso26021_2SendKeyValues
        );
        for i in 0x61..=0x7E {
            assert_eq!(
                SecurityAccessType::try_from(i).unwrap(),
                SecurityAccessType::SystemSupplierSpecific(i)
            );
        }
        assert_eq!(
            SecurityAccessType::try_from(0x7F).unwrap(),
            SecurityAccessType::IsoSaeReserved(0x7F)
        );
        for i in 0x80..=0xFF {
            match SecurityAccessType::try_from(i).unwrap_err() {
                Error::InvalidSecurityAccessType(value) => assert_eq!(value, i),
                _ => panic!("Invalid error type"),
            }
        }
    }

    #[test]
    fn a_level_is_named_by_its_request_seed() {
        const SEED: u8 = match SecurityAccessLevel::from_request_seed(0x03) {
            Ok(level) => level.request_seed(),
            Err(_) => 0xFF,
        };
        assert_eq!(SEED, 0x03);
        for value in 0x00..=0xFFu8 {
            match SecurityAccessLevel::from_request_seed(value) {
                Ok(level) => {
                    assert!(value % 2 == 1 && value <= 0x41, "{value:#04X}");
                    assert_eq!(level.request_seed(), value);
                    assert_eq!(level.send_key(), value + 1);
                }
                Err(error) => {
                    assert!(value % 2 == 0 || value > 0x41, "{value:#04X}");
                    assert!(
                        matches!(error, Error::InvalidSecurityAccessType(v) if v == value)
                    );
                }
            }
        }
    }

    /// A send-key built from a level encodes that level's even byte, so it cannot be
    /// mistaken for the seed request.
    #[test]
    fn seed_and_key_for_one_level_encode_the_pair() {
        let level = SecurityAccessLevel::from_request_seed(0x01).unwrap();
        assert_eq!(u8::from(SecurityAccessType::RequestSeed(level)), 0x01);
        assert_eq!(u8::from(SecurityAccessType::SendKey(level)), 0x02);
        assert_eq!(
            SecurityAccessType::try_from(0x02).unwrap(),
            SecurityAccessType::SendKey(level)
        );
    }

    #[test]
    fn security_access_type_round_trip_all_values() {
        for i in 0..=u8::MAX {
            let value = SecurityAccessType::try_from(i);
            match value {
                Ok(value) => assert_eq!(u8::from(value), i),
                Err(Error::InvalidSecurityAccessType(value)) => assert_eq!(value, i),
                _ => panic!("Invalid error type"),
            }
        }
    }
}

/// List of allowed [`NegativeResponseCode`] variants for the `SecurityAccess` service
const SECURITY_ACCESS_NEGATIVE_RESPONSE_CODES: [NegativeResponseCode; 8] = [
    NegativeResponseCode::SubFunctionNotSupported,
    NegativeResponseCode::IncorrectMessageLengthOrInvalidFormat,
    NegativeResponseCode::ConditionsNotCorrect,
    NegativeResponseCode::RequestSequenceError,
    NegativeResponseCode::RequestOutOfRange,
    NegativeResponseCode::InvalidKey,
    NegativeResponseCode::ExceedNumberOfAttempts,
    NegativeResponseCode::RequiredTimeDelayNotExpired,
];

/// Client request to unlock a security level. Borrows its data from the caller.
///
/// A `requestSeed` carries an optional, implementation-defined `securityAccessDataRecord`;
/// the server answers with a seed. A `sendKey` carries the key the client computed from
/// that seed; a positive response means the level is unlocked.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SecurityAccessRequest<'d> {
    /// Whether a positive response should be suppressed.
    pub suppress_positive_response: bool,
    /// The requested [`SecurityAccessType`].
    pub access_type: SecurityAccessType,
    /// The implementation-defined request data (seed data record or key bytes).
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub request_data: &'d [u8],
}

impl<'d> SecurityAccessRequest<'d> {
    /// Create a new security access request.
    #[must_use]
    pub const fn new(
        suppress_positive_response: bool,
        access_type: SecurityAccessType,
        request_data: &'d [u8],
    ) -> Self {
        Self {
            suppress_positive_response,
            access_type,
            request_data,
        }
    }

    /// Get the allowed [`NegativeResponseCode`] variants for this request
    #[must_use]
    pub fn allowed_nack_codes() -> &'static [NegativeResponseCode] {
        &SECURITY_ACCESS_NEGATIVE_RESPONSE_CODES
    }
}

impl Encode for SecurityAccessRequest<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let sub_function = SuppressablePositiveResponse::new(
            self.suppress_positive_response,
            self.access_type,
        );
        let mut written = write_u8(writer, u8::from(sub_function))?;
        written += write_bytes(writer, self.request_data)?;
        Ok(written)
    }
}

impl<'a> Decode<'a> for SecurityAccessRequest<'a> {
    type Error = crate::Error;

    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        let sub_function =
            SuppressablePositiveResponse::<SecurityAccessType>::try_from(buf[0])?;
        Ok((
            Self {
                suppress_positive_response: sub_function.suppress_positive_response(),
                access_type: sub_function.value(),
                request_data: &buf[1..],
            },
            &[],
        ))
    }
}

/// Zero-alloc response for security access. Borrows from the caller.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SecurityAccessResponse<'d> {
    /// The security access type echoed from the request.
    pub access_type: SecurityAccessType,
    /// The security seed bytes (empty for a `SendKey` positive response).
    #[cfg_attr(feature = "serde", serde(borrow))]
    pub security_seed: &'d [u8],
}

impl<'d> SecurityAccessResponse<'d> {
    /// Create a new security access response.
    #[must_use]
    pub const fn new(access_type: SecurityAccessType, security_seed: &'d [u8]) -> Self {
        Self {
            access_type,
            security_seed,
        }
    }
}

impl Encode for SecurityAccessResponse<'_> {
    type Error = crate::Error;

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, Error> {
        let mut written = write_u8(writer, u8::from(self.access_type))?;
        written += write_bytes(writer, self.security_seed)?;
        Ok(written)
    }
}

impl<'a> Decode<'a> for SecurityAccessResponse<'a> {
    type Error = crate::Error;

    /// # Errors
    ///
    /// ISO 14229-1:2020 Table 44 makes `securitySeed` mandatory after a `requestSeed` echo
    /// and absent after a `sendKey` one: [`Error::InsufficientData`] for a seed request
    /// answered without a seed, and [`Error::IncorrectMessageLengthOrInvalidFormat`] for a
    /// key answered with one. A system-supplier or reserved echo is not judged.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), Error> {
        if buf.is_empty() {
            return Err(Error::InsufficientData(Incomplete {
                needed: 1,
                available: buf.len(),
            }));
        }
        let access_type = SecurityAccessType::try_from(buf[0])?;
        let security_seed = &buf[1..];
        match access_type {
            SecurityAccessType::RequestSeed(_) | SecurityAccessType::Iso26021_2Values
                if security_seed.is_empty() =>
            {
                return Err(Error::InsufficientData(Incomplete {
                    needed: 2,
                    available: buf.len(),
                }));
            }
            SecurityAccessType::SendKey(_)
            | SecurityAccessType::Iso26021_2SendKeyValues
                if !security_seed.is_empty() =>
            {
                return Err(Error::IncorrectMessageLengthOrInvalidFormat);
            }
            _ => {}
        }
        Ok((
            Self {
                access_type,
                security_seed,
            },
            &[],
        ))
    }
}

#[cfg(test)]
mod request {
    use super::*;
    use crate::{
        Decode, Encode, test_util::assert_encode_size_agrees, test_util::assert_impl_eq,
    };

    #[test]
    fn derive_contract() {
        assert_impl_eq::<SecurityAccessRequest<'_>>();
        assert_impl_eq::<SecurityAccessResponse<'_>>();
        #[cfg(feature = "serde")]
        {
            use crate::test_util::assert_impl_serde;
            assert_impl_serde::<SecurityAccessRequest<'_>>();
            assert_impl_serde::<SecurityAccessResponse<'_>>();
        }
    }

    #[test]
    fn request_seed() {
        let bytes: [u8; 6] = [
            0x01, // requestSeed for the level whose seed request is 0x01
            0x00, 0x01, 0x02, 0x03, 0x04, // fake data
        ];
        let (req, _) = <SecurityAccessRequest as Decode>::decode(&bytes).unwrap();

        assert_eq!(
            req.access_type,
            SecurityAccessType::RequestSeed(SecurityAccessLevel(0x01))
        );
        assert_eq!(req.request_data, &[0x00, 0x01, 0x02, 0x03, 0x04]);

        let mut buf = [0u8; 16];
        let written =
            Encode::encode(&req, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        // The bytes, not just the count, so a swap of what `encode` writes fails.
        assert_eq!(&buf[..written], &bytes);
        assert_eq!(written, req.encoded_size().unwrap());
        assert_encode_size_agrees(&req);
    }
}

#[cfg(test)]
mod response {
    use super::*;
    use crate::{Decode, Encode, test_util::assert_encode_size_agrees};

    #[test]
    fn response_seed() {
        let bytes: [u8; 5] = [0x01, 0x00, 0x01, 0x02, 0x03];
        let (resp, _) = <SecurityAccessResponse as Decode>::decode(&bytes).unwrap();

        assert_eq!(
            resp.access_type,
            SecurityAccessType::RequestSeed(SecurityAccessLevel(0x01))
        );
        assert_eq!(resp.security_seed, &[0x00, 0x01, 0x02, 0x03]);

        let mut buf = [0u8; 16];
        let written =
            Encode::encode(&resp, &mut automotive_wire_codec::SliceSink::new(&mut buf))
                .unwrap();
        assert_eq!(&buf[..written], &bytes);
        assert_eq!(written, resp.encoded_size().unwrap());
        assert_encode_size_agrees(&resp);
    }

    /// ISO 14229-1:2020 Table 44 makes `securitySeed` mandatory where the request asked
    /// for one and absent where it sent a key, so `67 02 ..` does not decode its trailing
    /// bytes as a seed, and `67 01` with no seed is short.
    #[test]
    fn the_seed_is_present_exactly_for_a_seed_request() {
        let (resp, _) = <SecurityAccessResponse as Decode>::decode(&[0x02]).unwrap();
        assert_eq!(
            resp.access_type,
            SecurityAccessType::SendKey(SecurityAccessLevel(0x01))
        );
        assert_eq!(resp.security_seed, [0u8; 0]);
        let (resp, _) = <SecurityAccessResponse as Decode>::decode(&[0x60]).unwrap();
        assert_eq!(
            resp.access_type,
            SecurityAccessType::Iso26021_2SendKeyValues
        );

        for key_with_seed in [&[0x02, 0xAA][..], &[0x60, 0xAA]] {
            assert!(matches!(
                <SecurityAccessResponse as Decode>::decode(key_with_seed),
                Err(Error::IncorrectMessageLengthOrInvalidFormat)
            ));
        }
        for seed_without_seed in [&[0x01][..], &[0x5F]] {
            assert!(matches!(
                <SecurityAccessResponse as Decode>::decode(seed_without_seed),
                Err(Error::InsufficientData(_))
            ));
        }
        // A system-supplier sub-function's layout is the supplier's, so it is not judged.
        assert!(<SecurityAccessResponse as Decode>::decode(&[0x61]).is_ok());
        assert!(<SecurityAccessResponse as Decode>::decode(&[0x61, 0xAA]).is_ok());
    }
}
