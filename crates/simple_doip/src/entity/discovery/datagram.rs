//! Figure 16's generic header handler for a datagram on
//! `UDP_DISCOVERY`, Figure 13's vehicle identification request handler, and the frames
//! that answer them.

use core::net::{IpAddr, SocketAddr};

use super::{Facts, VehicleIdentity};
use crate::Vin;
use crate::messages::{
    EntityStatusNodeType, EntityStatusResponse, Header, Message, NackCode, Payload,
    PayloadType, ProtocolVersion, VehicleIdentificationResponse,
};
use crate::wire::{Decode, Encode, SliceSink};

/// The longest frame the entity sends on UDP: an announcement with its sync status.
pub(super) const FRAME_CAP: usize = Header::SIZE + 33;

/// What the entity owes a datagram, or its own announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Answer {
    Announcement,
    Identification,
    EntityStatus,
    PowerMode,
    Nack(NackCode),
}

/// When an [`Answer`] to a request is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum When {
    Now,
    /// After `A_DoIP_Announce_Wait` (8.DoIP-051).
    AfterAnnounceWait,
}

/// An answer owed to a datagram, in the protocol version it is sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Owed {
    pub(super) answer: Answer,
    pub(super) version: ProtocolVersion,
    pub(super) when: When,
}

impl Owed {
    const fn now(answer: Answer, version: ProtocolVersion) -> Self {
        Self {
            answer,
            version,
            when: When::Now,
        }
    }

    const fn nack(code: NackCode) -> Self {
        Self::now(Answer::Nack(code), ProtocolVersion::V2019)
    }
}

/// What a datagram of `length` bytes from `from`, of which `datagram` holds the first,
/// is owed, if anything.
pub(super) fn handle<I: VehicleIdentity>(
    datagram: &[u8],
    length: usize,
    from: SocketAddr,
    max_data_size: u32,
    identity: &I,
) -> Option<Owed> {
    let ignored_source = match from.ip() {
        IpAddr::V4(ip) => ip.is_broadcast() || ip.is_multicast(),
        IpAddr::V6(ip) => ip.is_multicast(),
    };
    if ignored_source || length < Header::SIZE {
        return None;
    }
    let Ok((header, payload)) = Header::decode(datagram) else {
        return Some(Owed::nack(NackCode::IncorrectPatternFormat));
    };
    if matches!(
        header.payload_type(),
        PayloadType::NegativeAcknowledge
            | PayloadType::VehicleAnnouncement
            | PayloadType::DoIPEntityStatusResponse
            | PayloadType::DiagnosticPowerModeInfoResponse
    ) {
        return None;
    }
    let identification = matches!(
        header.payload_type(),
        PayloadType::VehicleIdentificationRequest
            | PayloadType::VehicleIdentificationRequestWithEID
            | PayloadType::VehicleIdentificationRequestWithVIN
    );
    let version = match header.protocol_version() {
        version @ (ProtocolVersion::V2012 | ProtocolVersion::V2019) => version,
        ProtocolVersion::VehicleIdentificationRequest if identification => {
            ProtocolVersion::V2019
        }
        _ => return Some(Owed::nack(NackCode::IncorrectPatternFormat)),
    };
    let refused = |code| Some(Owed::now(Answer::Nack(code), version));
    let taken = identification
        || matches!(
            header.payload_type(),
            PayloadType::DoIPEntityStatusRequest
                | PayloadType::DiagnosticPowerModeInfoRequest
        );
    if !taken {
        return refused(NackCode::UnknownPayloadType);
    }
    if header.payload_length() > max_data_size {
        return refused(NackCode::MessageTooLarge);
    }
    let declared = usize::try_from(header.payload_length()).ok();
    let whole = header
        .payload_type()
        .allows_payload_length(header.payload_length())
        && declared.and_then(|declared| declared.checked_add(Header::SIZE)) == Some(length);
    if !whole {
        return refused(NackCode::InvalidPayloadLength);
    }
    let identified = Owed {
        answer: Answer::Identification,
        version,
        when: When::AfterAnnounceWait,
    };
    match Payload::decode(payload.get(..declared?)?, header.payload_type()).ok()? {
        Payload::VehicleIdentificationRequest => Some(identified),
        Payload::VehicleIdentificationRequestWithEid(eid) => {
            identity.matches_eid(&eid).then_some(identified)
        }
        Payload::VehicleIdentificationRequestWithVin(vin) => identity
            .vin()
            .is_some_and(|own| own.to_bytes() == vin)
            .then_some(identified),
        Payload::EntityStatusRequest => Some(Owed::now(Answer::EntityStatus, version)),
        Payload::PowerModeInfoRequest => Some(Owed::now(Answer::PowerMode, version)),
        _ => None,
    }
}

/// Encodes `answer` in `version` into `buf`, returning the frame.
pub(super) fn frame<'b, I: VehicleIdentity>(
    answer: Answer,
    version: ProtocolVersion,
    facts: Facts,
    identity: &I,
    buf: &'b mut [u8; FRAME_CAP],
) -> &'b [u8] {
    let identification = || VehicleIdentificationResponse {
        vin: identity.vin().map_or([0x00; 17], Vin::to_bytes),
        logical_address: facts.address,
        entity_id: identity.eid().to_bytes(),
        group_id: identity.gid(),
        further_action: identity.further_action(),
        vin_gid_sync_status: identity.sync_status(),
    };
    let payload = match answer {
        Answer::Announcement | Answer::Identification => {
            Payload::VehicleAnnouncement(identification())
        }
        Answer::EntityStatus => Payload::EntityStatusResponse(EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: facts.mcts,
            open_tcp_sockets: facts.open,
            max_data_size: Some(facts.max_data_size),
        }),
        Answer::PowerMode => Payload::PowerModeInfoResponse(identity.power_mode()),
        Answer::Nack(code) => Payload::DoIPNack(code),
    };
    let message = Message {
        protocol_version: version,
        payload,
    };
    let written = message.encode(&mut SliceSink::new(buf)).unwrap_or(0);
    buf.get(..written).unwrap_or_default()
}
