use core::fmt;

use crate::logical_address::LogicalAddress;

use automotive_wire_codec::{read_u8, read_u16_be, write_bytes, write_u8, write_u16_be};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// The code of a diagnostic message positive acknowledgement
/// (ISO 13400-2:2019 Table 24).
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
pub enum DiagnosticAckCode {
    /// `0x00`: the diagnostic message was received correctly and put into the
    /// transmission buffer of the destination network.
    RoutingConfirmationAck,
    /// A value Table 24 reserves.
    Reserved(u8),
}

impl From<u8> for DiagnosticAckCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => DiagnosticAckCode::RoutingConfirmationAck,
            _ => DiagnosticAckCode::Reserved(value),
        }
    }
}

impl From<DiagnosticAckCode> for u8 {
    fn from(value: DiagnosticAckCode) -> Self {
        match value {
            DiagnosticAckCode::RoutingConfirmationAck => 0x00,
            DiagnosticAckCode::Reserved(value) => value,
        }
    }
}

impl fmt::Debug for DiagnosticAckCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:#04X})", self, u8::from(*self))
    }
}

/// The code of a diagnostic message negative acknowledgement: why the message was
/// rejected (ISO 13400-2:2019 Table 26).
#[derive(Clone, Copy, strum::Display, Eq, PartialEq)]
pub enum DiagnosticNackCode {
    /// `0x02`: the source address is not the one routing activation registered on
    /// the connection.
    InvalidSourceAddress,
    /// `0x03`: the target address does not identify any ECU known to the `DoIP`
    /// entity.
    UnknownTargetAddress,
    /// `0x04`: the user data exceeds the maximum size this `DoIP` entity can forward.
    DiagnosticMessageTooLarge,
    /// `0x05`: the `DoIP` entity has insufficient buffer memory to accept the message.
    OutOfMemory,
    /// `0x06`: the target is known but cannot currently be reached.
    TargetUnreachable,
    /// `0x07`: the target's network is unknown.
    UnknownNetwork,
    /// `0x08`: forwarding the message onto the target's transport protocol failed.
    TransportProtocolError,
    /// A value Table 26 reserves.
    Reserved(u8),
}

impl From<u8> for DiagnosticNackCode {
    fn from(value: u8) -> Self {
        match value {
            0x02 => DiagnosticNackCode::InvalidSourceAddress,
            0x03 => DiagnosticNackCode::UnknownTargetAddress,
            0x04 => DiagnosticNackCode::DiagnosticMessageTooLarge,
            0x05 => DiagnosticNackCode::OutOfMemory,
            0x06 => DiagnosticNackCode::TargetUnreachable,
            0x07 => DiagnosticNackCode::UnknownNetwork,
            0x08 => DiagnosticNackCode::TransportProtocolError,
            _ => DiagnosticNackCode::Reserved(value),
        }
    }
}

impl From<DiagnosticNackCode> for u8 {
    fn from(value: DiagnosticNackCode) -> Self {
        match value {
            DiagnosticNackCode::InvalidSourceAddress => 0x02,
            DiagnosticNackCode::UnknownTargetAddress => 0x03,
            DiagnosticNackCode::DiagnosticMessageTooLarge => 0x04,
            DiagnosticNackCode::OutOfMemory => 0x05,
            DiagnosticNackCode::TargetUnreachable => 0x06,
            DiagnosticNackCode::UnknownNetwork => 0x07,
            DiagnosticNackCode::TransportProtocolError => 0x08,
            DiagnosticNackCode::Reserved(value) => value,
        }
    }
}

impl fmt::Debug for DiagnosticNackCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:#04X})", self, u8::from(*self))
    }
}

/// A diagnostic message positive acknowledgement ([`PayloadType`] `0x8002`,
/// ISO 13400-2:2019 Table 23), sent back to whoever transmitted the original
/// [`DiagnosticMessage`](super::DiagnosticMessage).
///
/// [`PayloadType`]: super::PayloadType::DiagnosticMessagePositiveAcknowledge
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DiagnosticMessageAck<'a> {
    /// Logical address of the entity sending this acknowledgement.
    pub source_address: LogicalAddress,
    /// Logical address of the entity this acknowledgement is addressed to (the
    /// original diagnostic message's sender).
    pub target_address: LogicalAddress,
    /// The acknowledgement code.
    pub ack_code: DiagnosticAckCode,
    /// The first bytes of the diagnostic message being acknowledged, echoed back
    /// so the sender can correlate this acknowledgement with its request.
    pub previous_message_data: &'a [u8],
}

/// Owned mirror of [`DiagnosticMessageAck`] for values that must outlive an RX buffer.
#[cfg(feature = "alloc")]
#[derive(Clone, Eq, PartialEq)]
pub struct OwnedDiagnosticMessageAck {
    /// Logical address of the entity sending this acknowledgement.
    pub source_address: LogicalAddress,
    /// Logical address of the entity this acknowledgement is addressed to (the
    /// original diagnostic message's sender).
    pub target_address: LogicalAddress,
    /// The acknowledgement code.
    pub ack_code: DiagnosticAckCode,
    /// The first bytes of the diagnostic message being acknowledged, echoed back
    /// so the sender can correlate this acknowledgement with its request.
    pub previous_message_data: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl OwnedDiagnosticMessageAck {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> DiagnosticMessageAck<'_> {
        DiagnosticMessageAck {
            source_address: self.source_address,
            target_address: self.target_address,
            ack_code: self.ack_code,
            previous_message_data: &self.previous_message_data,
        }
    }
}

#[cfg(feature = "alloc")]
impl DiagnosticMessageAck<'_> {
    /// Copy the borrowed `previous_message_data` into an owned buffer, detaching
    /// this acknowledgement from the RX buffer it was decoded from.
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedDiagnosticMessageAck {
        OwnedDiagnosticMessageAck {
            source_address: self.source_address,
            target_address: self.target_address,
            ack_code: self.ack_code,
            previous_message_data: self.previous_message_data.to_vec(),
        }
    }
}

impl<'a> Decode<'a> for DiagnosticMessageAck<'a> {
    type Error = MessageError;

    /// Deserialize a diagnostic message acknowledgement from a byte slice. Consumes the
    /// entire buffer; all bytes after the fixed fields are treated as the previous
    /// message data.
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        let (target_address, rest) = read_u16_be(rest)?;
        let (ack_code, rest) = read_u8(rest)?;

        // Remaining bytes are the previous message data; consume the whole buffer.
        let previous_message_data = rest;

        Ok((
            Self {
                source_address: LogicalAddress(source_address),
                target_address: LogicalAddress(target_address),
                ack_code: ack_code.into(),
                previous_message_data,
            },
            &[],
        ))
    }
}

impl Encode for DiagnosticMessageAck<'_> {
    type Error = MessageError;

    /// Closed form matching [`Self::encode`]: 2-byte source + 2-byte target + 1-byte ack
    /// code + previous message data.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(5 + self.previous_message_data.len())
    }

    /// Serialize this diagnostic message acknowledgement into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        write_u16_be(writer, self.target_address.into())?;
        write_u8(writer, self.ack_code.into())?;
        let previous_message_data = self.previous_message_data;
        write_bytes(writer, previous_message_data)?;
        Ok(5 + previous_message_data.len())
    }
}

impl fmt::Debug for DiagnosticMessageAck<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let previous_message_data = self.previous_message_data;
        f.debug_struct("DiagnosticMessageAck")
            .field("source_address", &self.source_address)
            .field("target_address", &self.target_address)
            .field("ack_code", &self.ack_code)
            .field(
                "previous_message_data",
                &format_args!(
                    "({} bytes): {:#04X?}",
                    previous_message_data.len(),
                    previous_message_data
                ),
            )
            .finish()
    }
}

#[cfg(feature = "alloc")]
impl fmt::Debug for OwnedDiagnosticMessageAck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

/// A diagnostic message negative acknowledgement ([`PayloadType`] `0x8003`,
/// ISO 13400-2:2019 Table 25): the original
/// [`DiagnosticMessage`](super::DiagnosticMessage) was rejected, and why.
///
/// [`PayloadType`]: super::PayloadType::DiagnosticMessageNegativeAcknowledge
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DiagnosticMessageNack<'a> {
    /// Logical address of the entity sending this acknowledgement.
    pub source_address: LogicalAddress,
    /// Logical address of the entity this acknowledgement is addressed to (the
    /// original diagnostic message's sender).
    pub target_address: LogicalAddress,
    /// Why the diagnostic message was rejected.
    pub nack_code: DiagnosticNackCode,
    /// The first bytes of the diagnostic message being acknowledged, echoed back
    /// so the sender can correlate this acknowledgement with its request.
    pub previous_message_data: &'a [u8],
}

/// Owned mirror of [`DiagnosticMessageNack`] for values that must outlive an RX buffer.
#[cfg(feature = "alloc")]
#[derive(Clone, Eq, PartialEq)]
pub struct OwnedDiagnosticMessageNack {
    /// Logical address of the entity sending this acknowledgement.
    pub source_address: LogicalAddress,
    /// Logical address of the entity this acknowledgement is addressed to (the
    /// original diagnostic message's sender).
    pub target_address: LogicalAddress,
    /// Why the diagnostic message was rejected.
    pub nack_code: DiagnosticNackCode,
    /// The first bytes of the diagnostic message being acknowledged, echoed back
    /// so the sender can correlate this acknowledgement with its request.
    pub previous_message_data: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl OwnedDiagnosticMessageNack {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> DiagnosticMessageNack<'_> {
        DiagnosticMessageNack {
            source_address: self.source_address,
            target_address: self.target_address,
            nack_code: self.nack_code,
            previous_message_data: &self.previous_message_data,
        }
    }
}

#[cfg(feature = "alloc")]
impl DiagnosticMessageNack<'_> {
    /// Copy the borrowed `previous_message_data` into an owned buffer, detaching
    /// this acknowledgement from the RX buffer it was decoded from.
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedDiagnosticMessageNack {
        OwnedDiagnosticMessageNack {
            source_address: self.source_address,
            target_address: self.target_address,
            nack_code: self.nack_code,
            previous_message_data: self.previous_message_data.to_vec(),
        }
    }
}

impl<'a> Decode<'a> for DiagnosticMessageNack<'a> {
    type Error = MessageError;

    /// Deserialize a diagnostic message negative acknowledgement from a byte slice.
    /// Consumes the entire buffer; all bytes after the fixed fields are treated as
    /// the previous message data.
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        let (target_address, rest) = read_u16_be(rest)?;
        let (nack_code, rest) = read_u8(rest)?;

        // Remaining bytes are the previous message data; consume the whole buffer.
        let previous_message_data = rest;

        Ok((
            Self {
                source_address: LogicalAddress(source_address),
                target_address: LogicalAddress(target_address),
                nack_code: nack_code.into(),
                previous_message_data,
            },
            &[],
        ))
    }
}

impl Encode for DiagnosticMessageNack<'_> {
    type Error = MessageError;

    /// Closed form matching [`Self::encode`]: 2-byte source + 2-byte target + 1-byte ack
    /// code + previous message data.
    ///
    /// # Errors
    /// Never returns an error; the size is always computable.
    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(5 + self.previous_message_data.len())
    }

    /// Serialize this diagnostic message negative acknowledgement into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        write_u16_be(writer, self.source_address.into())?;
        write_u16_be(writer, self.target_address.into())?;
        write_u8(writer, self.nack_code.into())?;
        let previous_message_data = self.previous_message_data;
        write_bytes(writer, previous_message_data)?;
        Ok(5 + previous_message_data.len())
    }
}

impl fmt::Debug for DiagnosticMessageNack<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let previous_message_data = self.previous_message_data;
        f.debug_struct("DiagnosticMessageNack")
            .field("source_address", &self.source_address)
            .field("target_address", &self.target_address)
            .field("nack_code", &self.nack_code)
            .field(
                "previous_message_data",
                &format_args!(
                    "({} bytes): {:#04X?}",
                    previous_message_data.len(),
                    previous_message_data
                ),
            )
            .finish()
    }
}

#[cfg(feature = "alloc")]
impl fmt::Debug for OwnedDiagnosticMessageNack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write as _;

    /// Minimal fixed-capacity `core::fmt::Write` sink so this test does not depend on
    /// `std`/`alloc`.
    struct FixedBuf {
        buf: [u8; 256],
        len: usize,
    }

    impl core::fmt::Write for FixedBuf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let end = self.len + bytes.len();
            if end > self.buf.len() {
                return Err(core::fmt::Error);
            }
            self.buf[self.len..end].copy_from_slice(bytes);
            self.len = end;
            Ok(())
        }
    }

    #[test]
    fn test_print() {
        let ack = DiagnosticMessageAck {
            source_address: LogicalAddress(0x1234),
            target_address: LogicalAddress(0x5678),
            ack_code: DiagnosticAckCode::Reserved(8),
            previous_message_data: &[0x01, 0x02, 0x03][..],
        };
        let mut out = FixedBuf {
            buf: [0u8; 256],
            len: 0,
        };
        write!(out, "{ack:?}").unwrap();
        assert!(out.len > 0);
    }
}
