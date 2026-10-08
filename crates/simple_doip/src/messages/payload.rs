use crate::messages::{
    AliveCheckResponse, DiagnosticMessage, DiagnosticMessageAck, DiagnosticMessageNack,
    DiagnosticPowerModeCode, EntityStatusResponse, MessageError, PayloadType,
    RoutingActivationResponse, VehicleIdentificationResponse,
};

use super::traits::{Decode, Encode};
use super::{NackCode, RoutingActivationRequest};
use automotive_wire_codec::{read_array, write_bytes};

/// Maps [`PayloadType`] to the corresponding `Payload` type when reading and writing
/// messages. This is the main payload type for `DoIP` messages.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Payload<'a> {
    /// Generic negative acknowledgement (`PayloadType::NegativeAcknowledge`, 0x0000):
    /// the header itself was rejected (e.g. bad payload type or length) rather than
    /// the diagnostic content within a valid message.
    DoIPNack(NackCode),
    /// Alive check request (`PayloadType::AliveCheckRequest`, 0x0007), sent by a
    /// `DoIP` entity to confirm a TCP connection is still active. Carries no data.
    AliveCheckRequest,
    /// Response to an alive check request (`PayloadType::AliveCheckResponse`, 0x0008),
    /// identifying the responding entity's logical address.
    AliveCheckResponse(AliveCheckResponse),
    /// A diagnostic message (`PayloadType::DiagnosticMessage`, 0x8001) carrying a
    /// UDS/diagnostic payload between tester and ECU, addressed by source and
    /// target logical address.
    DiagnosticMessage(DiagnosticMessage<'a>),
    /// Positive acknowledgement of a diagnostic message
    /// ([`PayloadType::DiagnosticMessagePositiveAcknowledge`]).
    DiagnosticMessageAck(DiagnosticMessageAck<'a>),
    /// Negative acknowledgement of a diagnostic message
    /// ([`PayloadType::DiagnosticMessageNegativeAcknowledge`]): it was rejected, and
    /// why.
    DiagnosticMessageNack(DiagnosticMessageNack<'a>),
    /// Request for the `DoIP` entity's status (`PayloadType::DoIPEntityStatusRequest`,
    /// 0x4001): how many diagnostic sockets are open versus the maximum supported.
    /// Carries no data.
    EntityStatusRequest,
    /// Response to an entity status request
    /// (`PayloadType::DoIPEntityStatusResponse`, 0x4002): node type, open/max
    /// socket counts, and the optional max data size.
    EntityStatusResponse(EntityStatusResponse),
    /// Request for the diagnostic power mode
    /// ([`PayloadType::DiagnosticPowerModeInfoRequest`]). Carries no data.
    PowerModeInfoRequest,
    /// Response to a diagnostic power mode information request
    /// (`PayloadType::DiagnosticPowerModeInfoResponse`, 0x4004): the vehicle's
    /// current power mode.
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    /// Request to activate routing on a TCP connection
    /// (`PayloadType::RoutingActivationRequest`, 0x0005), sent by the tester before
    /// diagnostic messages may be exchanged.
    RoutingActivationRequest(RoutingActivationRequest),
    /// Response to a routing activation request
    /// (`PayloadType::RoutingActivationResponse`, 0x0006), granting or denying
    /// diagnostic access on the connection.
    RoutingActivationResponse(RoutingActivationResponse),
    /// Vehicle announcement / vehicle identification response
    /// (`PayloadType::VehicleAnnouncement`, 0x0004). Shares the
    /// [`VehicleIdentificationResponse`] wire format.
    VehicleAnnouncement(VehicleIdentificationResponse),
    /// Request for vehicle identification from every entity that receives it
    /// ([`PayloadType::VehicleIdentificationRequest`]). Carries no data.
    VehicleIdentificationRequest,
    /// Request for vehicle identification from the entity with this entity ID
    /// ([`PayloadType::VehicleIdentificationRequestWithEID`]).
    VehicleIdentificationRequestWithEid([u8; 6]),
    /// Request for vehicle identification from the entities of the vehicle with this
    /// VIN ([`PayloadType::VehicleIdentificationRequestWithVIN`]).
    VehicleIdentificationRequestWithVin([u8; 17]),
    /// A directed reply to a specific vehicle identification request, as opposed
    /// to the unsolicited [`Payload::VehicleAnnouncement`]. ISO 13400-2 defines a
    /// single wire payload type (0x0004, "vehicle announcement/identification
    /// response message") for both uses, so this variant encodes identically to
    /// `VehicleAnnouncement` and [`Payload::decode`] never produces it directly —
    /// it exists for callers that want to express "this is a reply to a request"
    /// at construction time.
    VehicleIdentificationResponse(VehicleIdentificationResponse),
}

/// Owned mirror of [`Payload`] for values that must outlive an RX buffer (tokio
/// channels, `ServerConnectionHandler` responses). Only the two data-carrying leaf
/// variants need owned storage; every other variant is already fully owned.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OwnedPayload {
    /// Owned mirror of [`Payload::DoIPNack`].
    DoIPNack(NackCode),
    /// Owned mirror of [`Payload::AliveCheckRequest`].
    AliveCheckRequest,
    /// Owned mirror of [`Payload::AliveCheckResponse`].
    AliveCheckResponse(AliveCheckResponse),
    /// Owned mirror of [`Payload::DiagnosticMessage`]; owns its trailing
    /// diagnostic data instead of borrowing from an RX buffer.
    DiagnosticMessage(super::OwnedDiagnosticMessage),
    /// Owned mirror of [`Payload::DiagnosticMessageAck`]; owns its trailing
    /// previous-diagnostic-data bytes instead of borrowing from an RX buffer.
    DiagnosticMessageAck(super::OwnedDiagnosticMessageAck),
    /// Owned mirror of [`Payload::DiagnosticMessageNack`]; owns its trailing
    /// previous-diagnostic-data bytes instead of borrowing from an RX buffer.
    DiagnosticMessageNack(super::OwnedDiagnosticMessageNack),
    /// Owned mirror of [`Payload::EntityStatusRequest`].
    EntityStatusRequest,
    /// Owned mirror of [`Payload::EntityStatusResponse`].
    EntityStatusResponse(EntityStatusResponse),
    /// Owned mirror of [`Payload::PowerModeInfoRequest`].
    PowerModeInfoRequest,
    /// Owned mirror of [`Payload::PowerModeInfoResponse`].
    PowerModeInfoResponse(DiagnosticPowerModeCode),
    /// Owned mirror of [`Payload::RoutingActivationRequest`].
    RoutingActivationRequest(RoutingActivationRequest),
    /// Owned mirror of [`Payload::RoutingActivationResponse`].
    RoutingActivationResponse(RoutingActivationResponse),
    /// Vehicle announcement / vehicle identification response
    /// (`PayloadType::VehicleAnnouncement`, 0x0004). Shares the
    /// [`VehicleIdentificationResponse`] wire format.
    VehicleAnnouncement(VehicleIdentificationResponse),
    /// Owned mirror of [`Payload::VehicleIdentificationRequest`].
    VehicleIdentificationRequest,
    /// Owned mirror of [`Payload::VehicleIdentificationRequestWithEid`].
    VehicleIdentificationRequestWithEid([u8; 6]),
    /// Owned mirror of [`Payload::VehicleIdentificationRequestWithVin`].
    VehicleIdentificationRequestWithVin([u8; 17]),
    /// Owned mirror of [`Payload::VehicleIdentificationResponse`].
    VehicleIdentificationResponse(VehicleIdentificationResponse),
}

#[cfg(feature = "alloc")]
impl Payload<'_> {
    /// Copy any borrowed payload data into an owned payload.
    #[must_use]
    pub fn to_owned_payload(&self) -> OwnedPayload {
        match self {
            Payload::DoIPNack(nack) => OwnedPayload::DoIPNack(*nack),
            Payload::AliveCheckRequest => OwnedPayload::AliveCheckRequest,
            Payload::AliveCheckResponse(response) => {
                OwnedPayload::AliveCheckResponse(*response)
            }
            Payload::DiagnosticMessage(message) => {
                OwnedPayload::DiagnosticMessage(message.to_owned_message())
            }
            Payload::DiagnosticMessageAck(ack) => {
                OwnedPayload::DiagnosticMessageAck(ack.to_owned_message())
            }
            Payload::DiagnosticMessageNack(nack) => {
                OwnedPayload::DiagnosticMessageNack(nack.to_owned_message())
            }
            Payload::EntityStatusRequest => OwnedPayload::EntityStatusRequest,
            Payload::EntityStatusResponse(response) => {
                OwnedPayload::EntityStatusResponse(*response)
            }
            Payload::PowerModeInfoResponse(code) => {
                OwnedPayload::PowerModeInfoResponse(*code)
            }
            Payload::RoutingActivationRequest(request) => {
                OwnedPayload::RoutingActivationRequest(*request)
            }
            Payload::RoutingActivationResponse(response) => {
                OwnedPayload::RoutingActivationResponse(*response)
            }
            Payload::VehicleAnnouncement(response) => {
                OwnedPayload::VehicleAnnouncement(*response)
            }
            Payload::VehicleIdentificationRequest => {
                OwnedPayload::VehicleIdentificationRequest
            }
            Payload::VehicleIdentificationRequestWithEid(eid) => {
                OwnedPayload::VehicleIdentificationRequestWithEid(*eid)
            }
            Payload::VehicleIdentificationRequestWithVin(vin) => {
                OwnedPayload::VehicleIdentificationRequestWithVin(*vin)
            }
            Payload::PowerModeInfoRequest => OwnedPayload::PowerModeInfoRequest,
            Payload::VehicleIdentificationResponse(response) => {
                OwnedPayload::VehicleIdentificationResponse(*response)
            }
        }
    }
}

#[cfg(feature = "alloc")]
impl OwnedPayload {
    /// Cheap borrowed view for encode paths and read-only inspection.
    #[must_use]
    pub fn as_ref(&self) -> Payload<'_> {
        match self {
            OwnedPayload::DoIPNack(nack) => Payload::DoIPNack(*nack),
            OwnedPayload::AliveCheckRequest => Payload::AliveCheckRequest,
            OwnedPayload::AliveCheckResponse(response) => {
                Payload::AliveCheckResponse(*response)
            }
            OwnedPayload::DiagnosticMessage(message) => {
                Payload::DiagnosticMessage(message.as_ref())
            }
            OwnedPayload::DiagnosticMessageAck(ack) => {
                Payload::DiagnosticMessageAck(ack.as_ref())
            }
            OwnedPayload::DiagnosticMessageNack(nack) => {
                Payload::DiagnosticMessageNack(nack.as_ref())
            }
            OwnedPayload::EntityStatusRequest => Payload::EntityStatusRequest,
            OwnedPayload::EntityStatusResponse(response) => {
                Payload::EntityStatusResponse(*response)
            }
            OwnedPayload::PowerModeInfoResponse(code) => {
                Payload::PowerModeInfoResponse(*code)
            }
            OwnedPayload::RoutingActivationRequest(request) => {
                Payload::RoutingActivationRequest(*request)
            }
            OwnedPayload::RoutingActivationResponse(response) => {
                Payload::RoutingActivationResponse(*response)
            }
            OwnedPayload::VehicleAnnouncement(response) => {
                Payload::VehicleAnnouncement(*response)
            }
            OwnedPayload::VehicleIdentificationRequest => {
                Payload::VehicleIdentificationRequest
            }
            OwnedPayload::VehicleIdentificationRequestWithEid(eid) => {
                Payload::VehicleIdentificationRequestWithEid(*eid)
            }
            OwnedPayload::VehicleIdentificationRequestWithVin(vin) => {
                Payload::VehicleIdentificationRequestWithVin(*vin)
            }
            OwnedPayload::PowerModeInfoRequest => Payload::PowerModeInfoRequest,
            OwnedPayload::VehicleIdentificationResponse(response) => {
                Payload::VehicleIdentificationResponse(*response)
            }
        }
    }
}

impl<'a> Payload<'a> {
    /// Decode a payload of the given type from exactly the payload bytes of one message.
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the payload cannot be deserialized
    pub fn decode(buf: &'a [u8], payload_type: PayloadType) -> Result<Self, MessageError> {
        Ok(match payload_type {
            PayloadType::AliveCheckResponse => {
                Self::AliveCheckResponse(AliveCheckResponse::decode(buf)?.0)
            }
            PayloadType::NegativeAcknowledge => Self::DoIPNack(NackCode::decode(buf)?.0),
            PayloadType::VehicleIdentificationRequest => Self::VehicleIdentificationRequest,
            PayloadType::VehicleIdentificationRequestWithEID => {
                Self::VehicleIdentificationRequestWithEid(read_array::<6>(buf)?.0)
            }
            PayloadType::VehicleIdentificationRequestWithVIN => {
                Self::VehicleIdentificationRequestWithVin(read_array::<17>(buf)?.0)
            }
            PayloadType::DiagnosticPowerModeInfoRequest => Self::PowerModeInfoRequest,
            PayloadType::VehicleAnnouncement => {
                Self::VehicleAnnouncement(VehicleIdentificationResponse::decode(buf)?.0)
            }
            PayloadType::RoutingActivationRequest => {
                Self::RoutingActivationRequest(RoutingActivationRequest::decode(buf)?.0)
            }
            PayloadType::RoutingActivationResponse => {
                Self::RoutingActivationResponse(RoutingActivationResponse::decode(buf)?.0)
            }
            PayloadType::AliveCheckRequest => Self::AliveCheckRequest,
            PayloadType::DoIPEntityStatusRequest => Self::EntityStatusRequest,
            PayloadType::DoIPEntityStatusResponse => {
                Self::EntityStatusResponse(EntityStatusResponse::decode(buf)?.0)
            }
            PayloadType::DiagnosticPowerModeInfoResponse => {
                Self::PowerModeInfoResponse(DiagnosticPowerModeCode::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessage => {
                Self::DiagnosticMessage(DiagnosticMessage::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessagePositiveAcknowledge => {
                Self::DiagnosticMessageAck(DiagnosticMessageAck::decode(buf)?.0)
            }
            PayloadType::DiagnosticMessageNegativeAcknowledge => {
                Self::DiagnosticMessageNack(DiagnosticMessageNack::decode(buf)?.0)
            }
            PayloadType::Reserved(_) | PayloadType::ReservedVehicleManufacturer(_) => {
                return Err(MessageError::UnsupportedPayloadType(payload_type));
            }
        })
    }
}

impl Encode for Payload<'_> {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(match self {
            Payload::DoIPNack(nack) => nack.encoded_size()?,
            Payload::AliveCheckRequest
            | Payload::EntityStatusRequest
            | Payload::PowerModeInfoRequest
            | Payload::VehicleIdentificationRequest => 0,
            Payload::VehicleIdentificationRequestWithEid(eid) => eid.len(),
            Payload::VehicleIdentificationRequestWithVin(vin) => vin.len(),
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.encoded_size()?
            }
            Payload::DiagnosticMessage(diagnostic_message) => {
                diagnostic_message.encoded_size()?
            }
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.encoded_size()?
            }
            Payload::DiagnosticMessageNack(diagnostic_message_nack) => {
                diagnostic_message_nack.encoded_size()?
            }
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.encoded_size()?
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.encoded_size()?
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.encoded_size()?
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.encoded_size()?
            }
            // `VehicleAnnouncement` shares the `VehicleIdentificationResponse` wire format.
            Payload::VehicleIdentificationResponse(vehicle_identification_response)
            | Payload::VehicleAnnouncement(vehicle_identification_response) => {
                vehicle_identification_response.encoded_size()?
            }
        })
    }

    /// Serialize this payload into `writer`
    ///
    /// # Errors
    /// Returns a [`MessageError`] if the payload cannot be serialized
    fn encode(
        &self,
        writer: &mut impl automotive_wire_codec::Sink,
    ) -> Result<usize, MessageError> {
        Ok(match self {
            Payload::DoIPNack(nack) => nack.encode(writer)?,
            Payload::AliveCheckRequest
            | Payload::EntityStatusRequest
            | Payload::PowerModeInfoRequest
            | Payload::VehicleIdentificationRequest => 0,
            Payload::VehicleIdentificationRequestWithEid(eid) => {
                write_bytes(writer, eid)?;
                eid.len()
            }
            Payload::VehicleIdentificationRequestWithVin(vin) => {
                write_bytes(writer, vin)?;
                vin.len()
            }
            Payload::AliveCheckResponse(alive_check_response) => {
                alive_check_response.encode(writer)?
            }
            Payload::DiagnosticMessage(diagnostic_message) => {
                diagnostic_message.encode(writer)?
            }
            Payload::DiagnosticMessageAck(diagnostic_message_ack) => {
                diagnostic_message_ack.encode(writer)?
            }
            Payload::DiagnosticMessageNack(diagnostic_message_nack) => {
                diagnostic_message_nack.encode(writer)?
            }
            Payload::EntityStatusResponse(entity_status_response) => {
                entity_status_response.encode(writer)?
            }
            Payload::PowerModeInfoResponse(diagnostic_power_mode_code) => {
                diagnostic_power_mode_code.encode(writer)?
            }
            Payload::RoutingActivationRequest(routing_activation_request) => {
                routing_activation_request.encode(writer)?
            }
            Payload::RoutingActivationResponse(routing_activation_response) => {
                routing_activation_response.encode(writer)?
            }
            // `VehicleAnnouncement` shares the `VehicleIdentificationResponse` wire format.
            Payload::VehicleIdentificationResponse(vehicle_identification_response)
            | Payload::VehicleAnnouncement(vehicle_identification_response) => {
                vehicle_identification_response.encode(writer)?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalAddress;
    use crate::messages::{FurtherActionRequired, VinGidSyncStatus};
    use automotive_wire_codec::SliceSink;

    /// A peer sending a payload type we cannot decode (e.g. a reserved type) must return
    /// an error, never panic (regression test for `todo!()` decode arms).
    #[test]
    fn unsupported_payload_type_errors_not_panics() {
        // 0x0009 falls in the reserved range -> `PayloadType::Reserved`.
        let payload_type = PayloadType::from(0x0009u16);
        assert!(matches!(payload_type, PayloadType::Reserved(0x0009)));
        let result = Payload::decode(&[], payload_type);
        assert!(matches!(
            result,
            Err(MessageError::UnsupportedPayloadType(_))
        ));
    }

    /// ISO 13400-2:2019 Tables 24 and 26 give the positive and negative
    /// acknowledgements separate code tables, so the payload type alone says which
    /// one arrived and its code is read against that type's table: `0x00` under
    /// `0x8003` is a reserved negative code, and `0x03` under `0x8002` a reserved
    /// positive one.
    #[test]
    fn the_payload_type_decides_whether_an_acknowledgement_is_negative() {
        use crate::messages::{
            DiagnosticAckCode, DiagnosticMessageAck, DiagnosticMessageNack,
            DiagnosticNackCode,
        };
        let body = |code| [0x00, 0x01, 0x0E, 0x00, code];

        assert!(matches!(
            Payload::decode(
                &body(0x00),
                PayloadType::DiagnosticMessageNegativeAcknowledge
            ),
            Ok(Payload::DiagnosticMessageNack(DiagnosticMessageNack {
                nack_code: DiagnosticNackCode::Reserved(0x00),
                ..
            }))
        ));
        assert!(matches!(
            Payload::decode(
                &body(0x03),
                PayloadType::DiagnosticMessagePositiveAcknowledge
            ),
            Ok(Payload::DiagnosticMessageAck(DiagnosticMessageAck {
                ack_code: DiagnosticAckCode::Reserved(0x03),
                ..
            }))
        ));
    }

    /// Previously-`todo!()` decode arms are now implemented; verify they decode instead of
    /// panicking.
    #[test]
    fn entity_status_request_decodes_empty() {
        let payload = Payload::decode(&[], PayloadType::DoIPEntityStatusRequest).unwrap();
        assert!(matches!(payload, Payload::EntityStatusRequest));
    }

    /// ISO 13400-2:2019 Tables 3 and 4: the directed identification requests carry
    /// the EID or VIN they name, and survive a round trip.
    #[test]
    fn directed_identification_requests_keep_what_they_name() {
        let eid = [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF];
        let vin = *b"WVWZZZ1JZXW000001";
        for (payload_type, payload, body) in [
            (
                PayloadType::VehicleIdentificationRequestWithEID,
                Payload::VehicleIdentificationRequestWithEid(eid),
                &eid[..],
            ),
            (
                PayloadType::VehicleIdentificationRequestWithVIN,
                Payload::VehicleIdentificationRequestWithVin(vin),
                &vin[..],
            ),
        ] {
            assert_eq!(Payload::decode(body, payload_type).unwrap(), payload);
            let mut buf = [0u8; 32];
            let written = payload.encode(&mut SliceSink::new(&mut buf)).unwrap();
            assert_eq!(&buf[..written], body);
            assert_eq!(payload.encoded_size().unwrap(), body.len());
        }
    }

    /// A directed identification request too short for what it names is incomplete.
    #[test]
    fn a_short_directed_identification_request_is_incomplete() {
        assert!(matches!(
            Payload::decode(&[0; 5], PayloadType::VehicleIdentificationRequestWithEID),
            Err(MessageError::Incomplete(_))
        ));
        assert!(matches!(
            Payload::decode(&[0; 16], PayloadType::VehicleIdentificationRequestWithVIN),
            Err(MessageError::Incomplete(_))
        ));
    }

    /// ISO 13400-2:2019 Table 8: the power mode request carries no data.
    #[test]
    fn power_mode_request_decodes_empty() {
        let payload =
            Payload::decode(&[], PayloadType::DiagnosticPowerModeInfoRequest).unwrap();
        assert_eq!(payload, Payload::PowerModeInfoRequest);
        assert_eq!(payload.encoded_size().unwrap(), 0);
    }

    /// ISO 13400-2:2019 Tables 5 and 11: an entity may omit the sync status and the max
    /// data size, and what it sends still decodes.
    #[test]
    fn optional_trailing_fields_may_be_absent() {
        let status =
            Payload::decode(&[0x01, 0x01, 0x00], PayloadType::DoIPEntityStatusResponse)
                .unwrap();
        assert!(matches!(
            status,
            Payload::EntityStatusResponse(EntityStatusResponse {
                max_data_size: None,
                ..
            })
        ));
        let announcement =
            Payload::decode(&[0x30; 32], PayloadType::VehicleAnnouncement).unwrap();
        assert!(matches!(
            announcement,
            Payload::VehicleAnnouncement(VehicleIdentificationResponse {
                vin_gid_sync_status: None,
                ..
            })
        ));
    }

    /// `VehicleAnnouncement` must round-trip encode -> decode (regression test for the
    /// `todo!()` encode arms that panicked on a value decode had produced).
    #[test]
    fn vehicle_announcement_round_trips() {
        let response = VehicleIdentificationResponse {
            vin: [0x41; 17],
            logical_address: LogicalAddress(0x0E00),
            entity_id: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            group_id: Some([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: Some(VinGidSyncStatus::Synchronized),
        };
        let payload = Payload::VehicleAnnouncement(response);

        let mut buf = [0u8; 64];
        let written = {
            let mut writer = SliceSink::new(&mut buf);
            payload.encode(&mut writer).unwrap()
        };
        assert_eq!(written, payload.encoded_size().unwrap());

        let decoded =
            Payload::decode(&buf[..written], PayloadType::VehicleAnnouncement).unwrap();
        assert_eq!(decoded, payload);
    }
}
