//! `DoIP` message types: the generic header, the [`Payload`] enum covering every
//! `DoIP` payload type this crate supports, and the concrete request/response
//! structs for each one, per ISO 13400-2.
//!
//! A code enum's variant that carries the wire value, such as [`NackCode::Reserved`], is
//! built only by converting the value, so it never holds one a named variant has and `==`
//! agrees with the wire. Match it as `Reserved { .. }` and read the value back with the
//! enum's `From`:
//!
//! ```
//! use simple_doip::messages::NackCode;
//!
//! assert_eq!(NackCode::from(0x00), NackCode::IncorrectPatternFormat);
//! assert!(matches!(NackCode::from(0x7F), NackCode::Reserved { .. }));
//! assert_eq!(u8::from(NackCode::from(0x7F)), 0x7F);
//! ```
//!
//! ```compile_fail
//! let aliased = simple_doip::messages::NackCode::Reserved(0x00);
//! ```

mod alive_check_response;
pub use alive_check_response::AliveCheckResponse;
mod diagnostic_message;
pub use diagnostic_message::DiagnosticMessage;
#[cfg(feature = "alloc")]
pub use diagnostic_message::OwnedDiagnosticMessage;
mod diagnostic_message_ack;
pub use diagnostic_message_ack::{
    DiagnosticAckCode, DiagnosticMessageAck, DiagnosticMessageNack, DiagnosticNackCode,
};
#[cfg(feature = "alloc")]
pub use diagnostic_message_ack::{OwnedDiagnosticMessageAck, OwnedDiagnosticMessageNack};
mod entity_status_response;
pub use entity_status_response::{EntityStatusNodeType, EntityStatusResponse};
mod header;
pub use header::{Header, PayloadType, ProtocolVersion};
mod message_error;
pub use message_error::MessageError;
mod nack;
pub use nack::NackCode;
mod payload;
#[cfg(feature = "alloc")]
pub use payload::OwnedPayload;
pub use payload::Payload;
mod power_mode_info_response;
pub use power_mode_info_response::DiagnosticPowerModeCode;
mod routing_activation_request;
pub use routing_activation_request::{ActivationTypeCode, RoutingActivationRequest};
mod routing_activation_response;
pub use routing_activation_response::{
    RoutingActivationResponse, RoutingActivationResponseCode,
};
mod traits;
use traits::take;
pub use traits::{Decode, Encode};
mod vehicle_identification_response;
pub use vehicle_identification_response::{
    FurtherActionRequired, VehicleIdentificationResponse, VinGidSyncStatus,
};

use crate::LogicalAddress;

/// A `DoIP` message: the protocol version it is sent in, and its payload.
///
/// The generic header is derived from these when the message is encoded: the payload
/// type from [`Payload::payload_type`], the payload length from the payload's encoded
/// size. Payload data borrows from the RX buffer it was decoded from (zero-copy); use
/// `Message::to_owned_message` (requires the `alloc` feature) to detach it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    /// The protocol version of the message's generic header.
    pub protocol_version: ProtocolVersion,
    /// The decoded, type-specific `DoIP` payload.
    pub payload: Payload<'a>,
}

/// Fully owned `DoIP` message for values that must outlive an RX buffer, such as the
/// items the `codec` feature's `MessageCodec` yields.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedMessage {
    /// The protocol version of the message's generic header.
    pub protocol_version: ProtocolVersion,
    /// The decoded, type-specific `DoIP` payload, owning any trailing data instead of
    /// borrowing from an RX buffer.
    pub payload: OwnedPayload,
}

impl<'a> Message<'a> {
    /// Check whether the given payload type is a valid response to this message
    #[must_use]
    pub fn is_response(&self, payload_type: PayloadType) -> bool {
        match self.payload.payload_type() {
            PayloadType::RoutingActivationRequest => {
                payload_type == PayloadType::RoutingActivationResponse
            }
            PayloadType::AliveCheckRequest => {
                payload_type == PayloadType::AliveCheckResponse
            }
            PayloadType::DiagnosticMessage => {
                // DiagnosticMessage can be a request or response in certain models
                payload_type == PayloadType::DiagnosticMessageNegativeAcknowledge
                    || payload_type == PayloadType::DiagnosticMessagePositiveAcknowledge
                    || payload_type == PayloadType::DiagnosticMessage
            }
            PayloadType::DoIPEntityStatusRequest => {
                payload_type == PayloadType::DoIPEntityStatusResponse
            }
            PayloadType::DiagnosticPowerModeInfoRequest => {
                payload_type == PayloadType::DiagnosticPowerModeInfoResponse
            }
            PayloadType::VehicleIdentificationRequest
            | PayloadType::VehicleIdentificationRequestWithEID
            | PayloadType::VehicleIdentificationRequestWithVIN => {
                payload_type == PayloadType::VehicleAnnouncement
            }
            _ => false,
        }
    }

    /// The generic header this message is encoded with: its protocol version, its
    /// payload's [type](Payload::payload_type) and its payload's encoded size.
    ///
    /// # Errors
    /// Returns [`MessageError::PayloadTooLarge`] if the payload does not fit the `u32`
    /// payload length field.
    pub fn header(&self) -> Result<Header, MessageError> {
        let size = self.payload.encoded_size()?;
        let payload_length =
            u32::try_from(size).map_err(|_| MessageError::PayloadTooLarge { size })?;
        Ok(Header::new(
            self.protocol_version,
            self.payload.payload_type(),
            payload_length,
        ))
    }

    /// Construct an alive check request message
    #[must_use]
    pub fn alive_check_request(protocol_version: ProtocolVersion) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::AliveCheckRequest,
        }
    }

    /// Construct an alive check response message
    #[must_use]
    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::AliveCheckResponse(AliveCheckResponse { source_address }),
        }
    }

    /// Construct a diagnostic message carrying opaque user data, at least one byte of
    /// it: an empty `user_data` does not encode (ISO 13400-2:2019 Table 21).
    #[must_use]
    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        user_data: &'a [u8],
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::DiagnosticMessage(DiagnosticMessage {
                source_address,
                target_address,
                user_data,
            }),
        }
    }

    /// Construct a diagnostic message positive acknowledgment, with the
    /// [`DiagnosticAckCode::RoutingConfirmationAck`] code.
    #[must_use]
    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        previous_message_data: &'a [u8],
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::DiagnosticMessageAck(DiagnosticMessageAck {
                source_address,
                target_address,
                ack_code: DiagnosticAckCode::RoutingConfirmationAck,
                previous_message_data,
            }),
        }
    }

    /// Construct a diagnostic message negative acknowledgment.
    #[must_use]
    pub fn diagnostic_message_nack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        nack_code: DiagnosticNackCode,
        previous_message_data: &'a [u8],
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::DiagnosticMessageNack(DiagnosticMessageNack {
                source_address,
                target_address,
                nack_code,
                previous_message_data,
            }),
        }
    }

    /// Construct a routing activation request message
    #[must_use]
    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address,
                activation_type,
                reserved: [0, 0, 0, 0],
                reserved_vehicle_manufacturer,
            }),
        }
    }

    /// Construct a routing activation response message
    #[must_use]
    pub fn routing_activation_response(
        protocol_version: ProtocolVersion,
        logical_address_tester: LogicalAddress,
        logical_address_of_doip_entity: LogicalAddress,
        routing_activation_response_code: RoutingActivationResponseCode,
        reserved_oem: [u8; 4],
        oem_specific: Option<[u8; 4]>,
    ) -> Message<'a> {
        Message {
            protocol_version,
            payload: Payload::RoutingActivationResponse(RoutingActivationResponse {
                logical_address_tester,
                logical_address_of_doip_entity,
                routing_activation_response_code,
                reserved_oem,
                oem_specific,
            }),
        }
    }
}

impl<'a> Decode<'a> for Message<'a> {
    type Error = MessageError;

    /// Deserialize a complete `DoIP` message (header + payload) from a byte slice
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the header or payload cannot be deserialized
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (header, rest) = Header::decode(buf)?;
        let (payload_bytes, rest) = take(rest, header.payload_length() as usize)?;
        let payload = Payload::decode(payload_bytes, header.payload_type())?;
        Ok((
            Message {
                protocol_version: header.protocol_version(),
                payload,
            },
            rest,
        ))
    }
}

impl Encode for Message<'_> {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(Header::SIZE + self.payload.encoded_size()?)
    }

    /// Serialize this message, the [header](Message::header) it derives and then its
    /// payload, into `writer`.
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the header or payload cannot be
    /// serialized, or [`MessageError::PayloadTooLarge`] if the payload does
    /// not fit the `u32` length field.
    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        let written = self.header()?.encode(writer)?;
        Ok(written + self.payload.encode(writer)?)
    }
}

#[cfg(feature = "alloc")]
impl Message<'_> {
    /// Copy borrowed payload data into an owned message.
    #[must_use]
    pub fn to_owned_message(&self) -> OwnedMessage {
        OwnedMessage {
            protocol_version: self.protocol_version,
            payload: self.payload.to_owned_payload(),
        }
    }
}

#[cfg(feature = "alloc")]
impl OwnedMessage {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> Message<'_> {
        Message {
            protocol_version: self.protocol_version,
            payload: self.payload.as_ref(),
        }
    }

    /// Check whether the given payload type is a valid response to this message
    #[must_use]
    pub fn is_response(&self, payload_type: PayloadType) -> bool {
        self.as_ref().is_response(payload_type)
    }

    /// The generic header this message is encoded with, as [`Message::header`].
    ///
    /// # Errors
    /// Returns [`MessageError::PayloadTooLarge`] if the payload does not fit the `u32`
    /// payload length field.
    pub fn header(&self) -> Result<Header, MessageError> {
        self.as_ref().header()
    }

    /// Construct an alive check request message
    #[must_use]
    pub fn alive_check_request(protocol_version: ProtocolVersion) -> OwnedMessage {
        Message::alive_check_request(protocol_version).to_owned_message()
    }

    /// Construct an alive check response message
    #[must_use]
    pub fn alive_check_response(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
    ) -> OwnedMessage {
        Message::alive_check_response(protocol_version, source_address).to_owned_message()
    }

    /// Construct a routing activation request message
    #[must_use]
    pub fn routing_activation_request(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        activation_type: ActivationTypeCode,
        reserved_vehicle_manufacturer: Option<[u8; 4]>,
    ) -> OwnedMessage {
        Message::routing_activation_request(
            protocol_version,
            source_address,
            activation_type,
            reserved_vehicle_manufacturer,
        )
        .to_owned_message()
    }

    /// Construct a routing activation response message
    #[must_use]
    pub fn routing_activation_response(
        protocol_version: ProtocolVersion,
        logical_address_tester: LogicalAddress,
        logical_address_of_doip_entity: LogicalAddress,
        routing_activation_response_code: RoutingActivationResponseCode,
        reserved_oem: [u8; 4],
        oem_specific: Option<[u8; 4]>,
    ) -> OwnedMessage {
        Message::routing_activation_response(
            protocol_version,
            logical_address_tester,
            logical_address_of_doip_entity,
            routing_activation_response_code,
            reserved_oem,
            oem_specific,
        )
        .to_owned_message()
    }

    /// Construct a diagnostic message carrying owned user data, at least one byte of
    /// it: an empty `user_data` does not encode (ISO 13400-2:2019 Table 21).
    #[must_use]
    pub fn diagnostic_message(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        user_data: alloc::vec::Vec<u8>,
    ) -> OwnedMessage {
        OwnedMessage {
            protocol_version,
            payload: OwnedPayload::DiagnosticMessage(OwnedDiagnosticMessage {
                source_address,
                target_address,
                user_data,
            }),
        }
    }

    /// Construct a diagnostic message positive acknowledgment carrying owned data,
    /// with the [`DiagnosticAckCode::RoutingConfirmationAck`] code.
    #[must_use]
    pub fn diagnostic_message_ack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        previous_message_data: alloc::vec::Vec<u8>,
    ) -> OwnedMessage {
        OwnedMessage {
            protocol_version,
            payload: OwnedPayload::DiagnosticMessageAck(OwnedDiagnosticMessageAck {
                source_address,
                target_address,
                ack_code: DiagnosticAckCode::RoutingConfirmationAck,
                previous_message_data,
            }),
        }
    }

    /// Construct a diagnostic message negative acknowledgment carrying owned data.
    #[must_use]
    pub fn diagnostic_message_nack(
        protocol_version: ProtocolVersion,
        source_address: LogicalAddress,
        target_address: LogicalAddress,
        nack_code: DiagnosticNackCode,
        previous_message_data: alloc::vec::Vec<u8>,
    ) -> OwnedMessage {
        OwnedMessage {
            protocol_version,
            payload: OwnedPayload::DiagnosticMessageNack(OwnedDiagnosticMessageNack {
                source_address,
                target_address,
                nack_code,
                previous_message_data,
            }),
        }
    }

    /// Construct a directed reply to a vehicle identification request.
    ///
    /// The payload is [`OwnedPayload::VehicleIdentificationResponse`], sent under
    /// [`PayloadType::VehicleAnnouncement`] (0x0004) exactly as an announcement is,
    /// so a peer decoding these bytes gets [`Payload::VehicleAnnouncement`].
    #[must_use]
    pub fn vehicle_identification_response(
        protocol_version: ProtocolVersion,
        response: VehicleIdentificationResponse,
    ) -> OwnedMessage {
        OwnedMessage {
            protocol_version,
            payload: OwnedPayload::VehicleIdentificationResponse(response),
        }
    }
}

/// Encodes through the borrowed view, `Message<'_>: Encode`, so the owned and borrowed
/// forms share one wire implementation.
#[cfg(feature = "alloc")]
impl Encode for OwnedMessage {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        self.as_ref().encoded_size()
    }

    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        self.as_ref().encode(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automotive_wire_codec::SliceSink;
    use header::{PayloadType, ProtocolVersion};

    /// Known frames decode to their protocol version and payload type.
    #[test]
    fn test_valid_messages() {
        let buf: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
        let deserialized_message: Message<'_> = Message::decode(&buf).unwrap().0;
        assert_eq!(
            deserialized_message.protocol_version,
            ProtocolVersion::V2012
        );
        assert_eq!(
            deserialized_message.payload.payload_type(),
            PayloadType::NegativeAcknowledge
        );
        let buf: [u8; 8] = [0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        let deserialized_message: Message<'_> = Message::decode(&buf).unwrap().0;
        assert_eq!(
            deserialized_message.protocol_version,
            ProtocolVersion::V2010
        );
        assert_eq!(
            deserialized_message.payload,
            Payload::VehicleIdentificationRequest
        );
    }
    #[test]
    fn test_invalid_inverse() {
        let buf: [u8; 8] = [0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // This parsing should error for the bad inverse

        assert!(matches!(
            Message::decode(&buf),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }

    /// Frames a message's encoded bytes back out and decodes them, as a sans-io caller
    /// does, checking the frame takes exactly the bytes written.
    fn reframe(message: &Message<'_>) {
        let mut buf = [0u8; 64];
        let written = message.encode(&mut SliceSink::new(&mut buf)).unwrap();
        let (frame, consumed) = crate::try_frame(&buf[..written]).unwrap().unwrap();
        assert_eq!(consumed, written);
        assert_eq!(frame.header, message.header().unwrap());
        let decoded = Message {
            protocol_version: frame.header.protocol_version(),
            payload: Payload::decode(frame.payload, frame.header.payload_type()).unwrap(),
        };
        assert_eq!(&decoded, message);
    }

    /// Encode a message into a stack buffer and frame it back out again, without any
    /// `Vec`/`alloc` involved, through the `no_std` / `no_alloc` API surface.
    #[test]
    fn test_no_std_stack_buffer_roundtrip() {
        let message: Message<'_> = Message::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            &[0x10u8, 0x02][..],
        );
        assert_eq!(message.header().unwrap().payload_length(), 6);
        reframe(&message);
    }

    /// `Message::diagnostic_message_ack` and `diagnostic_message_nack`: pin each
    /// header's payload type and length, and round-trip them the same way as the
    /// stack-buffer test above, without any `Vec`/`alloc`.
    #[test]
    fn test_diagnostic_message_ack_and_nack_round_trip() {
        let echoed = [0x10u8, 0x02];
        let ack = Message::diagnostic_message_ack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            &echoed[..],
        );
        let nack = Message::diagnostic_message_nack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            DiagnosticNackCode::UnknownTargetAddress,
            &echoed[..],
        );
        // Table 17 gives them 0x8002 and 0x8003.
        for (message, payload_type) in [
            (ack, PayloadType::DiagnosticMessagePositiveAcknowledge),
            (nack, PayloadType::DiagnosticMessageNegativeAcknowledge),
        ] {
            // 5 fixed bytes (two 2-byte addresses + 1 code byte) + 2 previous-message
            // bytes.
            let header = message.header().unwrap();
            assert_eq!(header.payload_length(), 7);
            assert_eq!(header.payload_type(), payload_type);
            reframe(&message);
        }
    }

    /// A routing activation response carrying `oem_specific` is sent with a payload
    /// length of the 13 bytes `encode` writes, so the framed bytes round-trip.
    #[test]
    fn test_routing_activation_response_oem_specific_round_trip() {
        let message: Message<'_> = Message::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        // 9 fixed bytes + 4 oem-specific bytes.
        assert_eq!(message.header().unwrap().payload_length(), 13);
        reframe(&message);
    }

    /// The header a message is sent with names its payload's type, whatever else the
    /// message holds: there is no second copy of the type to disagree with it.
    #[test]
    fn the_header_takes_its_payload_type_from_the_payload() {
        let message = Message::alive_check_request(ProtocolVersion::V2019);
        let header = message.header().unwrap();
        assert_eq!(header.payload_type(), PayloadType::AliveCheckRequest);
        assert_eq!(header.payload_length(), 0);
        assert_eq!(header.protocol_version(), ProtocolVersion::V2019);
        let mut buf = [0u8; 8];
        message.encode(&mut SliceSink::new(&mut buf)).unwrap();
        assert_eq!(buf, [0x03, 0xFC, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00]);
    }

    /// Table 17 gives the announcement and the identification response one payload
    /// type, 0x0004: both are sent under it, and either decodes as an announcement.
    #[test]
    fn an_identification_response_is_sent_as_an_announcement() {
        let response = VehicleIdentificationResponse {
            vin: [0x41; 17],
            logical_address: LogicalAddress(0x1000),
            entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            group_id: None,
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: Some(VinGidSyncStatus::Synchronized),
        };
        let announcement = Message {
            protocol_version: ProtocolVersion::V2019,
            payload: Payload::VehicleAnnouncement(response),
        };
        let identification = Message {
            protocol_version: ProtocolVersion::V2019,
            payload: Payload::VehicleIdentificationResponse(response),
        };
        let mut sent = [[0u8; 64]; 2];
        let mut written = [0; 2];
        for ((message, buf), written) in [&announcement, &identification]
            .into_iter()
            .zip(&mut sent)
            .zip(&mut written)
        {
            assert_eq!(
                message.header().unwrap().payload_type(),
                PayloadType::VehicleAnnouncement
            );
            *written = message.encode(&mut SliceSink::new(buf)).unwrap();
        }
        assert_eq!(sent[0][..written[0]], sent[1][..written[1]]);
        let (decoded, rest) = Message::decode(&sent[1][..written[1]]).unwrap();
        assert_eq!(rest, []);
        assert_eq!(decoded, announcement);
    }
}

/// The borrowed<->owned conversions and the owned/borrowed constructor header
/// agreement, as permanent regressions. Alloc-gated because
/// `to_owned_payload`/`to_owned_message` and the `Owned*` constructors only exist
/// under `feature = "alloc"`.
#[cfg(all(test, feature = "alloc"))]
mod alloc_conversion_tests {
    use super::*;
    use crate::GroupId;
    use crate::messages::{
        AliveCheckResponse, DiagnosticAckCode, DiagnosticMessage, DiagnosticMessageAck,
        DiagnosticMessageNack, DiagnosticNackCode, DiagnosticPowerModeCode,
        EntityStatusNodeType, EntityStatusResponse, FurtherActionRequired, NackCode,
        RoutingActivationResponseCode, VehicleIdentificationResponse, VinGidSyncStatus,
    };
    use alloc::vec::Vec;

    /// Construct one representative value of every `Payload<'_>` variant and assert that
    /// round-tripping through `to_owned_payload` / `OwnedPayload::as_ref` reproduces the
    /// original exactly. A future variant that is dropped from, or mis-mapped in, either
    /// conversion match will fail this test.
    #[test]
    fn payload_conversion_roundtrip_all_variants() {
        let diag_data = [0x10u8, 0x02];
        let ack_data = [0x3Eu8, 0x00, 0xAA];

        let values: Vec<Payload<'_>> = alloc::vec![
            Payload::DoIPNack(NackCode::IncorrectPatternFormat),
            Payload::AliveCheckRequest,
            Payload::AliveCheckResponse(AliveCheckResponse {
                source_address: LogicalAddress(0x0E00),
            }),
            Payload::DiagnosticMessage(DiagnosticMessage {
                source_address: LogicalAddress(0x0E00),
                target_address: LogicalAddress(0x1000),
                user_data: &diag_data[..],
            }),
            Payload::DiagnosticMessageAck(DiagnosticMessageAck {
                source_address: LogicalAddress(0xFFFF),
                target_address: LogicalAddress(0x0001),
                ack_code: DiagnosticAckCode::RoutingConfirmationAck,
                previous_message_data: &ack_data[..],
            }),
            Payload::DiagnosticMessageNack(DiagnosticMessageNack {
                source_address: LogicalAddress(0xFFFF),
                target_address: LogicalAddress(0x0001),
                nack_code: DiagnosticNackCode::TransportProtocolError,
                previous_message_data: &ack_data[..],
            }),
            Payload::EntityStatusRequest,
            Payload::EntityStatusResponse(EntityStatusResponse {
                node_type: EntityStatusNodeType::DoIPGateway,
                max_concurrent_tcp_sockets: 4,
                open_tcp_sockets: 0,
                max_data_size: Some(0x0000_FFFF),
            }),
            Payload::PowerModeInfoRequest,
            Payload::PowerModeInfoResponse(DiagnosticPowerModeCode::Ready),
            Payload::RoutingActivationRequest(RoutingActivationRequest {
                source_address: LogicalAddress(0x0E00),
                activation_type: ActivationTypeCode::Default,
                reserved: [0, 0, 0, 0],
                reserved_vehicle_manufacturer: Some([0xDE, 0xAD, 0xBE, 0xEF]),
            }),
            Payload::RoutingActivationResponse(RoutingActivationResponse {
                logical_address_tester: LogicalAddress(0x0E00),
                logical_address_of_doip_entity: LogicalAddress(0x1000),
                routing_activation_response_code:
                    RoutingActivationResponseCode::RoutingSuccessfullyActivated,
                reserved_oem: [0, 0, 0, 0],
                oem_specific: Some([0xDE, 0xAD, 0xBE, 0xEF]),
            }),
            Payload::VehicleAnnouncement(VehicleIdentificationResponse {
                vin: [0x41; 17],
                logical_address: LogicalAddress(0x0E00),
                entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
                group_id: GroupId::new([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]).ok(),
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: Some(VinGidSyncStatus::Synchronized),
            }),
            Payload::VehicleIdentificationRequest,
            Payload::VehicleIdentificationRequestWithEid([0x02, 0, 0, 0, 0, 1]),
            Payload::VehicleIdentificationRequestWithVin(*b"1HGCM82633A004352"),
            Payload::VehicleIdentificationResponse(VehicleIdentificationResponse {
                vin: *b"1HGCM82633A004352",
                logical_address: LogicalAddress(0x1000),
                entity_id: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
                group_id: None,
                further_action: FurtherActionRequired::NoFurtherActionRequired,
                vin_gid_sync_status: Some(VinGidSyncStatus::Synchronized),
            }),
        ];

        assert_eq!(values.len(), 17, "expected one value per Payload variant");

        for p in &values {
            let owned = p.to_owned_payload();
            assert_eq!(&owned.as_ref(), p, "conversion mismatch for {p:?}");
        }
    }

    /// Owned constructors must produce the header their borrowed counterpart does for
    /// the same arguments, its payload length included.
    #[test]
    fn owned_message_matches_borrowed_headers() {
        // routing_activation_response WITH oem_specific = Some(_) (13-byte case).
        let borrowed = Message::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        let owned = OwnedMessage::routing_activation_response(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            LogicalAddress(0x1000),
            RoutingActivationResponseCode::RoutingSuccessfullyActivated,
            [0x00, 0x00, 0x00, 0x00],
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        assert_eq!(borrowed.header().unwrap(), owned.header().unwrap());
        assert_eq!(borrowed.header().unwrap().payload_length(), 13);

        // diagnostic_message with non-empty data.
        let data = [0x22u8, 0xF1, 0x90, 0x00];
        let borrowed = Message::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0xE400),
            LogicalAddress(0x00FF),
            &data[..],
        );
        let owned = OwnedMessage::diagnostic_message(
            ProtocolVersion::V2012,
            LogicalAddress(0xE400),
            LogicalAddress(0x00FF),
            data.to_vec(),
        );
        assert_eq!(borrowed.header().unwrap(), owned.header().unwrap());
        assert_eq!(borrowed.to_owned_message().as_ref(), borrowed);

        // diagnostic_message_ack and _nack: ISO 13400-2:2019 Table 17 gives them 0x8002
        // and 0x8003.
        let echoed = [0x10u8, 0x03];
        let borrowed = Message::diagnostic_message_ack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0001),
            LogicalAddress(0x0E00),
            &echoed[..],
        );
        let owned = OwnedMessage::diagnostic_message_ack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0001),
            LogicalAddress(0x0E00),
            echoed.to_vec(),
        );
        assert_eq!(borrowed.header().unwrap(), owned.header().unwrap());
        assert_eq!(
            borrowed.header().unwrap().payload_type(),
            PayloadType::DiagnosticMessagePositiveAcknowledge
        );
        let borrowed = Message::diagnostic_message_nack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0001),
            LogicalAddress(0x0E00),
            DiagnosticNackCode::UnknownTargetAddress,
            &echoed[..],
        );
        let owned = OwnedMessage::diagnostic_message_nack(
            ProtocolVersion::V2012,
            LogicalAddress(0x0001),
            LogicalAddress(0x0E00),
            DiagnosticNackCode::UnknownTargetAddress,
            echoed.to_vec(),
        );
        assert_eq!(borrowed.header().unwrap(), owned.header().unwrap());
        assert_eq!(
            borrowed.header().unwrap().payload_type(),
            PayloadType::DiagnosticMessageNegativeAcknowledge
        );

        // routing_activation_request with a Some tail.
        let borrowed = Message::routing_activation_request(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            ActivationTypeCode::Default,
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        let owned = OwnedMessage::routing_activation_request(
            ProtocolVersion::V2012,
            LogicalAddress(0x0E00),
            ActivationTypeCode::Default,
            Some([0xDE, 0xAD, 0xBE, 0xEF]),
        );
        assert_eq!(borrowed.header().unwrap(), owned.header().unwrap());
    }
}
