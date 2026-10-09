//! Vehicle discovery for a tester (ISO 13400-2:2019 6.2, 7.4 to 7.6): finding the
//! entities on a network, and asking one its status or the vehicle's power mode, over a
//! UDP socket the integrator binds.
//!
//! Bind the socket on every address and a port in the dynamic range 49152 to 65535
//! (REQ 4.DoIP-135), able to send to the limited broadcast address; `edge-nal-std` binds
//! so. Each call sends its request, and listens on the same socket for the answers within
//! `A_DoIP_Ctrl`, 2 s (Table 12, REQ 4.DoIP-136).
//!
//! An identification request carries the default protocol version, `0xFF`, which an
//! entity takes whichever edition it implements (REQ 7.DoIP-156), so entities of earlier
//! editions are found too; the other requests carry this edition's, `0x03`.
//!
//! A [`Found`] entity is reached over `TCP_DATA` with
//! [`Tester::connect`](super::Tester::connect) at [`Found::tcp_address`].

use core::net::{IpAddr, Ipv4Addr, SocketAddr};

use edge_nal::{Readable, UdpReceive, UdpSend, UdpSplit};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Timer};

use crate::messages::{
    DiagnosticPowerModeCode, EntityStatusResponse, Header, Message, NackCode, Payload,
    PayloadType, ProtocolVersion, VehicleIdentificationResponse,
};
use crate::wire::{Decode, Encode, SliceSink};
use crate::{EntityId, TCP_PORT, UDP_DISCOVERY_PORT, Vin};

/// `A_DoIP_Ctrl`: how long a tester waits for the answers to a UDP request
/// (ISO 13400-2:2019 Table 12).
pub const A_DOIP_CTRL: Duration = Duration::from_secs(2);

/// Where a request to every entity on the network goes: the IPv4 limited broadcast
/// address, on `UDP_DISCOVERY`.
pub const BROADCAST: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), UDP_DISCOVERY_PORT);

/// The longest datagram a tester reads: an identification response with its sync status.
const RX_CAP: usize = Header::SIZE + 33;

/// Which entities a vehicle identification request asks to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Every entity that receives it (ISO 13400-2:2019 Table 2).
    All,
    /// The entity with this entity ID (Table 3). An entity may not support it.
    Eid(EntityId),
    /// The entities of the vehicle with this VIN (Table 4).
    Vin(Vin),
}

/// An entity that answered a vehicle identification request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    from: SocketAddr,
    identification: VehicleIdentificationResponse,
}

impl Found {
    /// The address the entity answered from.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.from
    }

    /// Where the entity takes `TCP_DATA` connections: its address, on
    /// [`TCP_PORT`].
    #[must_use]
    pub const fn tcp_address(&self) -> SocketAddr {
        SocketAddr::new(self.from.ip(), TCP_PORT)
    }

    /// What the entity identified itself as, its logical address included.
    #[must_use]
    pub const fn identification(&self) -> &VehicleIdentificationResponse {
        &self.identification
    }
}

/// Why a request to one entity was not answered.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryError<E> {
    /// Sending or receiving on the socket failed.
    #[error("the UDP socket failed: {0:?}")]
    Io(E),
    /// The entity refused the request with a generic header NACK, such as
    /// [`NackCode::UnknownPayloadType`] from one that does not support entity status.
    #[error("the entity refused the request: {0:?}")]
    Refused(NackCode),
    /// Nothing answered within [`A_DOIP_CTRL`].
    #[error("no answer within A_DoIP_Ctrl")]
    NoAnswer,
}

/// Asks the entities at `to` to identify themselves, and collects into `found` each one
/// that answers within [`A_DOIP_CTRL`]: an entity answering twice is kept once.
///
/// Returns how many it kept, `n`: the first `n` of `found` are [`Some`], in the order the
/// entities answered, and the rest [`None`]. An entity answering once `found` is full is
/// not kept. Waits the whole [`A_DOIP_CTRL`], as many entities may answer one request
/// (Figure 7).
///
/// # Arguments
///
/// * `socket` - the tester's bound socket; see the [module](self) docs.
/// * `to` - [`BROADCAST`] for every entity on the network, or one entity's address on
///   [`UDP_DISCOVERY_PORT`].
/// * `request` - which entities are to answer.
/// * `found` - where the entities that answered are kept.
///
/// # Errors
///
/// The socket's error where it fails. A datagram that is not an identification
/// response is ignored, and no answer at all is `Ok(0)`.
///
/// # Cancel safety
///
/// Dropping it loses the answers not yet in `found`.
pub async fn identify<U: UdpSplit>(
    socket: &mut U,
    to: SocketAddr,
    request: Request,
    found: &mut [Option<Found>],
) -> Result<usize, U::Error> {
    let payload = match request {
        Request::All => Payload::VehicleIdentificationRequest,
        Request::Eid(eid) => Payload::VehicleIdentificationRequestWithEid(eid.to_bytes()),
        Request::Vin(vin) => Payload::VehicleIdentificationRequestWithVin(vin.to_bytes()),
    };
    let payload_type = match request {
        Request::All => PayloadType::VehicleIdentificationRequest,
        Request::Eid(_) => PayloadType::VehicleIdentificationRequestWithEID,
        Request::Vin(_) => PayloadType::VehicleIdentificationRequestWithVIN,
    };
    found.fill(None);
    let collect = |from, message: Message<'_>| {
        if let Payload::VehicleAnnouncement(identification) = message.payload {
            let again = found.iter().flatten().any(|known| {
                known.from == from
                    && known.identification.entity_id == identification.entity_id
            });
            if !again && let Some(slot) = found.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(Found {
                    from,
                    identification,
                });
            }
        }
        None::<()>
    };
    let header = Header::new(
        ProtocolVersion::VehicleIdentificationRequest,
        payload_type,
        0,
    );
    exchange(socket, to, header, payload, collect).await?;
    Ok(found.iter().flatten().count())
}

/// Asks the entity at `entity` for its status (ISO 13400-2:2019 7.6), which includes the
/// largest payload it takes: the longest diagnostic message's user data is that less the
/// 4 bytes of its addresses.
///
/// # Arguments
///
/// * `socket` - the tester's bound socket; see the [module](self) docs.
/// * `entity` - the entity's address, on [`UDP_DISCOVERY_PORT`]: a [`Found::address`].
///
/// # Errors
///
/// - [`DiscoveryError::Io`] where the socket fails.
/// - [`DiscoveryError::Refused`] where the entity answers with a header NACK: an
///   entity need not support entity status.
/// - [`DiscoveryError::NoAnswer`] where it does not answer within [`A_DOIP_CTRL`].
pub async fn entity_status<U: UdpSplit>(
    socket: &mut U,
    entity: SocketAddr,
) -> Result<EntityStatusResponse, DiscoveryError<U::Error>> {
    ask(
        socket,
        entity,
        PayloadType::DoIPEntityStatusRequest,
        Payload::EntityStatusRequest,
        |payload| match payload {
            Payload::EntityStatusResponse(status) => Some(status),
            _ => None,
        },
    )
    .await
}

/// Asks the entity at `entity` for the vehicle's diagnostic power mode
/// (ISO 13400-2:2019 7.5).
///
/// # Arguments
///
/// * `socket` - the tester's bound socket; see the [module](self) docs.
/// * `entity` - the entity's address, on [`UDP_DISCOVERY_PORT`]: a [`Found::address`].
///
/// # Errors
///
/// - [`DiscoveryError::Io`] where the socket fails.
/// - [`DiscoveryError::Refused`] where the entity answers with a header NACK.
/// - [`DiscoveryError::NoAnswer`] where it does not answer within [`A_DOIP_CTRL`].
pub async fn power_mode<U: UdpSplit>(
    socket: &mut U,
    entity: SocketAddr,
) -> Result<DiagnosticPowerModeCode, DiscoveryError<U::Error>> {
    ask(
        socket,
        entity,
        PayloadType::DiagnosticPowerModeInfoRequest,
        Payload::PowerModeInfoRequest,
        |payload| match payload {
            Payload::PowerModeInfoResponse(mode) => Some(mode),
            _ => None,
        },
    )
    .await
}

/// Sends one request to `entity` and returns the first answer from it that `answer`
/// takes, or its NACK.
async fn ask<U: UdpSplit, T>(
    socket: &mut U,
    entity: SocketAddr,
    payload_type: PayloadType,
    payload: Payload<'static>,
    answer: impl Fn(Payload<'_>) -> Option<T>,
) -> Result<T, DiscoveryError<U::Error>> {
    let take = |from: SocketAddr, message: Message<'_>| {
        if from.ip() != entity.ip() {
            return None;
        }
        match message.payload {
            Payload::DoIPNack(code) => Some(Err(DiscoveryError::Refused(code))),
            payload => answer(payload).map(Ok),
        }
    };
    let header = Header::new(ProtocolVersion::V2019, payload_type, 0);
    exchange(socket, entity, header, payload, take)
        .await
        .map_err(DiscoveryError::Io)?
        .unwrap_or(Err(DiscoveryError::NoAnswer))
}

/// Sends `payload` under `header` to `to`, then hands `take` each frame received within
/// [`A_DOIP_CTRL`] until it returns something.
async fn exchange<U: UdpSplit, T>(
    socket: &mut U,
    to: SocketAddr,
    header: Header,
    payload: Payload<'_>,
    mut take: impl FnMut(SocketAddr, Message<'_>) -> Option<T>,
) -> Result<Option<T>, U::Error> {
    let (mut receiver, mut sender) = socket.split();
    let message = Message { header, payload };
    let mut frame = [0u8; Header::SIZE + 17];
    let written = message
        .encode(&mut SliceSink::new(&mut frame))
        .unwrap_or_default();
    sender
        .send(to, frame.get(..written).unwrap_or_default())
        .await?;
    let listen = async {
        let mut buf = [0u8; RX_CAP];
        loop {
            receiver.readable().await?;
            let (length, from) = receiver.receive(&mut buf).await?;
            let Some(datagram) = buf.get(..length) else {
                continue;
            };
            if let Ok((message, [])) = Message::decode(datagram)
                && let Some(taken) = take(from, message)
            {
                return Ok(taken);
            }
        }
    };
    match select(listen, Timer::after(A_DOIP_CTRL)).await {
        Either::First(taken) => taken.map(Some),
        Either::Second(()) => Ok(None),
    }
}
