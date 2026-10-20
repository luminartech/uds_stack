//! The `DoIP` generic header (Table 16): [`Header`], its [`ProtocolVersion`] and its
//! [`PayloadType`]. Decoding checks the inverse protocol version against the version.
use core::fmt::Debug;

use automotive_wire_codec::{
    read_u8, read_u16_be, read_u32_be, write_u8, write_u16_be, write_u32_be,
};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// The protocol version a `DoIP` header carries (Table 16).
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
#[repr(u8)]
pub enum ProtocolVersion {
    /// Reserved by ISO 13400-2; not a valid version for a `DoIP` entity to send.
    Reserved = 0x00,
    /// ISO/DIS 13400-2:2010
    V2010 = 0x01,
    /// ISO 13400-2:2012
    V2012 = 0x02,
    /// ISO 13400-2:2019
    V2019 = 0x03,
    /// Reserved for future editions, `0x04` to `0xFE`.
    #[non_exhaustive]
    ReservedFuture(u8),
    /// The default value a vehicle identification request carries, `0xFF`.
    VehicleIdentificationRequest = 0xFF,
}

impl ProtocolVersion {
    /// The inverse protocol version a header with this version carries: its bitwise
    /// complement.
    #[must_use]
    pub fn inverse(&self) -> u8 {
        match self {
            ProtocolVersion::Reserved => 0xFF,
            ProtocolVersion::V2010 => 0xFE,
            ProtocolVersion::V2012 => 0xFD,
            ProtocolVersion::V2019 => 0xFC,
            ProtocolVersion::ReservedFuture(value) => !value,
            ProtocolVersion::VehicleIdentificationRequest => 0x00,
        }
    }
}

impl From<u8> for ProtocolVersion {
    fn from(value: u8) -> Self {
        match value {
            0x00 => ProtocolVersion::Reserved,
            0x01 => ProtocolVersion::V2010,
            0x02 => ProtocolVersion::V2012,
            0x03 => ProtocolVersion::V2019,
            0x04..=0xFE => ProtocolVersion::ReservedFuture(value),
            0xFF => ProtocolVersion::VehicleIdentificationRequest,
        }
    }
}

impl From<ProtocolVersion> for u8 {
    fn from(value: ProtocolVersion) -> Self {
        match value {
            ProtocolVersion::Reserved => 0x00,
            ProtocolVersion::V2010 => 0x01,
            ProtocolVersion::V2012 => 0x02,
            ProtocolVersion::V2019 => 0x03,
            ProtocolVersion::ReservedFuture(value) => value,
            ProtocolVersion::VehicleIdentificationRequest => 0xFF,
        }
    }
}

impl Debug for ProtocolVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val: u8 = (*self).into();
        write!(f, "{self} ({val:#04X})")
    }
}

/// The payload type a `DoIP` header carries (Table 17).
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
#[repr(u16)]
pub enum PayloadType {
    /// Generic `DoIP` header negative acknowledge.
    NegativeAcknowledge = 0x0000,
    /// Vehicle identification request.
    VehicleIdentificationRequest = 0x0001,
    /// Vehicle identification request with entity ID (EID).
    VehicleIdentificationRequestWithEID = 0x0002,
    /// Vehicle identification request with vehicle identification number (VIN).
    VehicleIdentificationRequestWithVIN = 0x0003,
    /// Vehicle announcement, and vehicle identification response.
    VehicleAnnouncement = 0x0004,
    /// Routing activation request.
    RoutingActivationRequest = 0x0005,
    /// Routing activation response.
    RoutingActivationResponse = 0x0006,
    /// Alive check request.
    AliveCheckRequest = 0x0007,
    /// Alive check response.
    AliveCheckResponse = 0x0008,
    /// `DoIP` entity status request.
    DoIPEntityStatusRequest = 0x4001,
    /// `DoIP` entity status response.
    DoIPEntityStatusResponse = 0x4002,
    /// Diagnostic power mode information request.
    DiagnosticPowerModeInfoRequest = 0x4003,
    /// Diagnostic power mode information response.
    DiagnosticPowerModeInfoResponse = 0x4004,
    /// Diagnostic message.
    DiagnosticMessage = 0x8001,
    /// Diagnostic message positive acknowledge.
    DiagnosticMessagePositiveAcknowledge = 0x8002,
    /// Diagnostic message negative acknowledge.
    DiagnosticMessageNegativeAcknowledge = 0x8003,
    /// Reserved by ISO 13400-2.
    #[non_exhaustive]
    Reserved(u16),
    /// Reserved for the vehicle manufacturer, `0xF000` to `0xFFFF`.
    #[non_exhaustive]
    ReservedVehicleManufacturer(u16),
}

impl From<u16> for PayloadType {
    fn from(value: u16) -> Self {
        match value {
            0x0000 => PayloadType::NegativeAcknowledge,
            0x0001 => PayloadType::VehicleIdentificationRequest,
            0x0002 => PayloadType::VehicleIdentificationRequestWithEID,
            0x0003 => PayloadType::VehicleIdentificationRequestWithVIN,
            0x0004 => PayloadType::VehicleAnnouncement,
            0x0005 => PayloadType::RoutingActivationRequest,
            0x0006 => PayloadType::RoutingActivationResponse,
            0x0007 => PayloadType::AliveCheckRequest,
            0x0008 => PayloadType::AliveCheckResponse,
            0x4001 => PayloadType::DoIPEntityStatusRequest,
            0x4002 => PayloadType::DoIPEntityStatusResponse,
            0x4003 => PayloadType::DiagnosticPowerModeInfoRequest,
            0x4004 => PayloadType::DiagnosticPowerModeInfoResponse,
            0x8001 => PayloadType::DiagnosticMessage,
            0x8002 => PayloadType::DiagnosticMessagePositiveAcknowledge,
            0x8003 => PayloadType::DiagnosticMessageNegativeAcknowledge,
            0xF000..=0xFFFF => PayloadType::ReservedVehicleManufacturer(value),
            _ => PayloadType::Reserved(value),
        }
    }
}

impl From<PayloadType> for u16 {
    fn from(value: PayloadType) -> Self {
        match value {
            PayloadType::NegativeAcknowledge => 0x0000,
            PayloadType::VehicleIdentificationRequest => 0x0001,
            PayloadType::VehicleIdentificationRequestWithEID => 0x0002,
            PayloadType::VehicleIdentificationRequestWithVIN => 0x0003,
            PayloadType::VehicleAnnouncement => 0x0004,
            PayloadType::RoutingActivationRequest => 0x0005,
            PayloadType::RoutingActivationResponse => 0x0006,
            PayloadType::AliveCheckRequest => 0x0007,
            PayloadType::AliveCheckResponse => 0x0008,
            PayloadType::DoIPEntityStatusRequest => 0x4001,
            PayloadType::DoIPEntityStatusResponse => 0x4002,
            PayloadType::DiagnosticPowerModeInfoRequest => 0x4003,
            PayloadType::DiagnosticPowerModeInfoResponse => 0x4004,
            PayloadType::DiagnosticMessage => 0x8001,
            PayloadType::DiagnosticMessagePositiveAcknowledge => 0x8002,
            PayloadType::DiagnosticMessageNegativeAcknowledge => 0x8003,
            PayloadType::Reserved(value)
            | PayloadType::ReservedVehicleManufacturer(value) => value,
        }
    }
}

impl PayloadType {
    /// Whether a payload of `length` bytes has a length this payload type allows, the
    /// check REQ 7.DoIP-045 has an entity answer with generic header NACK `0x04`.
    ///
    /// The lengths are those of each payload's structure: Tables 2 to 5 and 8 to 11
    /// for the vehicle identification, power mode and entity status messages, Table 18
    /// for the generic NACK, Tables 21, 23 and 25 for the diagnostic message and its
    /// acknowledgments, Tables 27 and 28 for the alive check, and Tables 46 and 48 for
    /// routing activation. A diagnostic message's user data is mandatory (Table 21),
    /// which this crate reads as at least one byte. Always `true` for a reserved type,
    /// whose payload this crate does not model.
    #[must_use]
    pub const fn allows_payload_length(self, length: u32) -> bool {
        match self {
            PayloadType::VehicleIdentificationRequest
            | PayloadType::AliveCheckRequest
            | PayloadType::DoIPEntityStatusRequest
            | PayloadType::DiagnosticPowerModeInfoRequest => length == 0,
            PayloadType::NegativeAcknowledge
            | PayloadType::DiagnosticPowerModeInfoResponse => length == 1,
            PayloadType::AliveCheckResponse => length == 2,
            PayloadType::VehicleIdentificationRequestWithEID => length == 6,
            PayloadType::VehicleIdentificationRequestWithVIN => length == 17,
            PayloadType::VehicleAnnouncement => matches!(length, 32 | 33),
            PayloadType::RoutingActivationRequest => matches!(length, 7 | 11),
            PayloadType::RoutingActivationResponse => matches!(length, 9 | 13),
            PayloadType::DoIPEntityStatusResponse => matches!(length, 3 | 7),
            PayloadType::DiagnosticMessage
            | PayloadType::DiagnosticMessagePositiveAcknowledge
            | PayloadType::DiagnosticMessageNegativeAcknowledge => length >= 5,
            PayloadType::Reserved(_) | PayloadType::ReservedVehicleManufacturer(_) => true,
        }
    }
}

impl Debug for PayloadType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val: u16 = (*self).into();
        write!(f, "{self} ({val:#04X})")
    }
}

/// `DoIP` generic header (Table 16): 8 bytes, as read from the wire or to be written.
///
/// [`ProtocolVersion`] (1 byte), its inverse (1 byte, derived on encode and checked on
/// decode), [`PayloadType`] (2 bytes) and the payload length (4 bytes). A
/// [`Message`](crate::messages::Message) derives its header from its payload when
/// encoded, so a `Header` is built by hand only to frame bytes this crate does not
/// model.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Header {
    protocol_version: ProtocolVersion,
    payload_type: PayloadType,
    payload_length: u32,
}

impl Header {
    /// Size in bytes of an encoded header.
    pub const SIZE: usize = 8;

    /// A header for a payload of `payload_type` and `payload_length` bytes, sent in
    /// `protocol_version`.
    #[must_use]
    pub const fn new(
        protocol_version: ProtocolVersion,
        payload_type: PayloadType,
        payload_length: u32,
    ) -> Self {
        Header {
            protocol_version,
            payload_type,
            payload_length,
        }
    }

    /// The protocol version the message is sent in.
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// The type of the payload that follows the header.
    #[must_use]
    pub const fn payload_type(&self) -> PayloadType {
        self.payload_type
    }

    /// The length in bytes of the payload that follows the header, not counting the
    /// header itself.
    #[must_use]
    pub const fn payload_length(&self) -> u32 {
        self.payload_length
    }
}

impl<'a> Decode<'a> for Header {
    type Error = MessageError;

    /// Decodes a `DoIP` header from the front of `buf`.
    ///
    /// # Errors
    /// Returns [`MessageError::VersionInverseIncorrect`] if the inverse protocol version
    /// does not match, or [`MessageError::Incomplete`] if `buf` is too short.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (protocol_version, rest) = read_u8(buf)?;
        let protocol_version = ProtocolVersion::from(protocol_version);
        let (inverse_protocol_version, rest) = read_u8(rest)?;
        let (payload_type, rest) = read_u16_be(rest)?;
        let (payload_length, rest) = read_u32_be(rest)?;
        let expected = protocol_version.inverse();
        if inverse_protocol_version != expected {
            return Err(MessageError::VersionInverseIncorrect {
                expected,
                value: inverse_protocol_version,
            });
        }
        Ok((
            Header::new(protocol_version, payload_type.into(), payload_length),
            rest,
        ))
    }
}

impl Encode for Header {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(Header::SIZE)
    }

    /// Encodes this header into `writer`.
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        write_u8(writer, self.protocol_version.into())?;
        write_u8(writer, self.protocol_version.inverse())?;
        write_u16_be(writer, self.payload_type.into())?;
        write_u32_be(writer, self.payload_length)?;
        Ok(Header::SIZE)
    }
}

impl Debug for Header {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Header")
            .field("Version", &self.protocol_version)
            .field("payload_type", &self.payload_type)
            .field("payload_length", &self.payload_length)
            .finish()
    }
}
