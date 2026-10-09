//! `Entity` with discovery against a scripted UDP socket: vehicle announcement and
//! identification (7.4, Figure 13), diagnostic power mode (7.5), entity
//! status (7.6), and Figure 16's generic header handler on `UDP_DISCOVERY`.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::cell::RefCell;
use std::net::SocketAddr;
use std::pin::pin;

use embassy_time::Duration;
use embedded_io_async::ErrorKind;
use simple_doip::entity::{
    Discovery, Entity, EntityAddress, FixedIdentity, VehicleIdentity,
};
use simple_doip::messages::{
    DiagnosticPowerModeCode, EntityStatusNodeType, EntityStatusResponse,
    FurtherActionRequired, Message, NackCode, Payload, PayloadType, ProtocolVersion,
    VehicleIdentificationResponse, VinGidSyncStatus,
};
use simple_doip::service::DiagnosticEntity;
use simple_doip::wire::Decode;
use simple_doip::{EntityId, GroupId, LogicalAddress, UDP_DISCOVERY_PORT, Vin};
use support::mock_stack::{
    ENTITY, MockStack, MockUdp, MockUdpSocket, advance, clock, raw, until_stalled,
    versioned,
};
use support::the_tester;

type Sensor<'a, I = FixedIdentity> =
    Entity<'a, MockStack, 1, 4096, 1, Discovery<MockUdpSocket, I>>;

const EID: [u8; 6] = [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF];
const VIN: [u8; 17] = *b"WVWZZZ1JZXW000001";
const SEED: u32 = 0x1234_5678;
/// `MAX_MESSAGE` less the generic header.
const MDS: u32 = 4088;

fn tester_at() -> SocketAddr {
    ([192, 168, 0, 2], 50_000).into()
}

fn broadcast() -> SocketAddr {
    ([255, 255, 255, 255], UDP_DISCOVERY_PORT).into()
}

fn vin() -> Vin {
    Vin::new(VIN).unwrap()
}

fn identity() -> FixedIdentity {
    FixedIdentity::new(EntityId::new(EID).unwrap(), DiagnosticPowerModeCode::Ready)
}

fn sensor<'a, I: VehicleIdentity>(
    stack: &'a MockStack,
    udp: &MockUdp,
    identity: I,
) -> Sensor<'a, I> {
    sensor_seeded(stack, udp, identity, SEED)
}

fn sensor_seeded<'a, I: VehicleIdentity>(
    stack: &'a MockStack,
    udp: &MockUdp,
    identity: I,
    seed: u32,
) -> Sensor<'a, I> {
    let address = EntityAddress::new(ENTITY, LogicalAddress(0xE400)).unwrap();
    Entity::new(stack, address, the_tester()).with_discovery(udp.socket(), identity, seed)
}

/// Sets the clock back to zero, for a second entity in one test.
fn restart_clock() {
    embassy_time::MockDriver::get().reset();
}

/// [`identity`], keeping the kind of each socket failure it is told of.
#[derive(Debug)]
struct Telling<'f> {
    identity: FixedIdentity,
    failures: &'f RefCell<Vec<ErrorKind>>,
}

impl<'f> Telling<'f> {
    fn new(failures: &'f RefCell<Vec<ErrorKind>>) -> Self {
        Self {
            identity: identity(),
            failures,
        }
    }
}

impl VehicleIdentity for Telling<'_> {
    fn vin(&self) -> Option<Vin> {
        self.identity.vin()
    }

    fn eid(&self) -> EntityId {
        self.identity.eid()
    }

    fn gid(&self) -> Option<GroupId> {
        self.identity.gid()
    }

    fn further_action(&self) -> FurtherActionRequired {
        self.identity.further_action()
    }

    fn sync_status(&self) -> Option<VinGidSyncStatus> {
        self.identity.sync_status()
    }

    fn power_mode(&self) -> DiagnosticPowerModeCode {
        self.identity.power_mode()
    }

    fn discovery_failed<E: embedded_io_async::Error>(&self, error: &E) {
        self.failures.borrow_mut().push(error.kind());
    }
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// Runs `next_event` until it waits; UDP never produces an event.
fn settle<E: DiagnosticEntity>(entity: &mut E)
where
    E::Error: std::fmt::Debug,
{
    let mut buf = [0u8; 64];
    let event = until_stalled(pin!(entity.next_event(&mut buf, None)));
    assert!(
        event.is_none(),
        "UDP raised an event: {:?}",
        event.map(|event| event.map(|_| ()))
    );
}

/// Advances the clock a millisecond at a time for `millis`, settling at each, and returns
/// what was sent with the milliseconds since the start it was sent at.
fn run_for<I: VehicleIdentity>(
    entity: &mut Sensor<'_, I>,
    udp: &MockUdp,
    millis: u64,
) -> Vec<(u64, SocketAddr, Vec<u8>)> {
    let mut sent = Vec::new();
    for at in 0..=millis {
        if at > 0 {
            advance(ms(1));
        }
        settle(entity);
        sent.extend(
            udp.take_sent()
                .into_iter()
                .map(|(to, frame)| (at, to, frame)),
        );
    }
    sent
}

/// The entity, past its three announcements.
fn announced<'a, I: VehicleIdentity>(
    stack: &'a MockStack,
    udp: &MockUdp,
    identity: I,
) -> Sensor<'a, I> {
    let mut entity = sensor(stack, udp, identity);
    assert_eq!(run_for(&mut entity, udp, 1600).len(), 3);
    entity
}

/// A request from the tester, with what it was answered and how many milliseconds after.
fn ask<I: VehicleIdentity>(
    entity: &mut Sensor<'_, I>,
    udp: &MockUdp,
    datagram: &[u8],
) -> Vec<(u64, SocketAddr, Vec<u8>)> {
    udp.deliver(tester_at(), datagram);
    let sent = run_for(entity, udp, 600);
    assert!(udp.all_received());
    sent
}

/// The one frame sent in answer, which must have gone to the tester.
fn answer(sent: &[(u64, SocketAddr, Vec<u8>)]) -> &[u8] {
    match sent {
        [(_, to, frame)] => {
            assert_eq!(*to, tester_at(), "an answer goes to the request's source");
            frame
        }
        _ => panic!("expected one answer, got {sent:?}"),
    }
}

fn first_sent(sent: &[(u64, SocketAddr, Vec<u8>)]) -> &(u64, SocketAddr, Vec<u8>) {
    sent.first().expect("something was sent")
}

fn not_all_equal(waits: &[u64]) -> bool {
    let mut distinct = waits.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    distinct.len() > 1
}

fn decode(frame: &[u8]) -> Message<'_> {
    Message::decode(frame).unwrap().0
}

fn nack(sent: &[(u64, SocketAddr, Vec<u8>)]) -> NackCode {
    match decode(answer(sent)).payload {
        Payload::DoIPNack(code) => code,
        other => panic!("expected a NACK, got {other:?}"),
    }
}

fn identification(frame: &[u8]) -> VehicleIdentificationResponse {
    let message = decode(frame);
    assert_eq!(
        message.header.payload_type,
        PayloadType::VehicleAnnouncement
    );
    match message.payload {
        Payload::VehicleAnnouncement(response) => response,
        other => panic!("expected an identification response, got {other:?}"),
    }
}

fn identification_request() -> Vec<u8> {
    raw(0x0001, &[])
}

// --- the announcement ------------------------------------------------------------------

/// REQ 8.DoIP-050 and 125, Table 12, Figure 11: after `A_DoIP_Announce_Wait`, at most
/// 500 ms, three announcements `A_DoIP_Announce_Interval`, 500 ms, apart, to the limited
/// broadcast address on `UDP_DISCOVERY`; and no more.
#[test]
fn three_announcements_follow_the_announce_wait() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = sensor(&stack, &udp, identity());

    let sent = run_for(&mut entity, &udp, 5000);

    let times: Vec<u64> = sent.iter().map(|(at, ..)| *at).collect();
    let &[first, second, third] = times.as_slice() else {
        panic!("expected three announcements, got {times:?}");
    };
    assert!(first <= 500, "{times:?}");
    assert_eq!(Some(second), first.checked_add(500));
    assert_eq!(Some(third), second.checked_add(500));
    for (_, to, frame) in &sent {
        assert_eq!(*to, broadcast());
        assert_eq!(
            decode(frame).header.protocol_version,
            ProtocolVersion::V2019
        );
        assert_eq!(identification(frame).logical_address, ENTITY);
    }
}

/// Table 12: `A_DoIP_Announce_Wait` is random, so entities seeded differently do not
/// announce together.
#[test]
fn the_announce_wait_follows_the_seed() {
    let _clock = clock();
    let first_at = |seed| {
        let (stack, udp) = (MockStack::new(4096), MockUdp::new());
        let mut entity = sensor_seeded(&stack, &udp, identity(), seed);
        let at = first_sent(&run_for(&mut entity, &udp, 500)).0;
        restart_clock();
        at
    };
    let waits: Vec<u64> = [1, 2, 3, 4].map(first_at).to_vec();
    assert!(not_all_equal(&waits), "{waits:?}");
}

/// Tables 1 and 5: what the announcement carries. A VIN not programmed
/// is all `0x00`, a GID not set likewise, and the optional sync status is sent only where
/// the identity has one.
#[test]
fn the_announcement_carries_the_identity() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut bare = sensor(&stack, &udp, identity());
    let sent = run_for(&mut bare, &udp, 500);
    let frame = &first_sent(&sent).2;
    assert_eq!(frame.len(), 8 + 32, "no sync status byte");
    assert_eq!(
        identification(frame),
        VehicleIdentificationResponse {
            vin: [0x00; 17],
            logical_address: ENTITY,
            entity_id: EID,
            group_id: None,
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: None,
        }
    );

    restart_clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let full = identity()
        .with_vin(vin())
        .with_gid(GroupId::new([1, 2, 3, 4, 5, 6]).unwrap())
        .with_sync_status(VinGidSyncStatus::Incomplete);
    let mut entity = sensor(&stack, &udp, full);
    let sent = run_for(&mut entity, &udp, 500);
    assert_eq!(
        identification(&first_sent(&sent).2),
        VehicleIdentificationResponse {
            vin: VIN,
            logical_address: ENTITY,
            entity_id: EID,
            group_id: Some([1, 2, 3, 4, 5, 6]),
            further_action: FurtherActionRequired::NoFurtherActionRequired,
            vin_gid_sync_status: Some(VinGidSyncStatus::Incomplete),
        }
    );
}

// --- vehicle identification (Figure 13) --------------------------------------------------

/// REQ 8.DoIP-046 and 051, 4.DoIP-137: a vehicle identification request is answered once,
/// after `A_DoIP_Announce_Wait`, to its source address and port, in its own protocol
/// version.
#[test]
fn an_identification_request_is_answered_after_the_announce_wait() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    let sent = ask(
        &mut entity,
        &udp,
        &versioned(identification_request(), 0x02),
    );

    let frame = answer(&sent);
    assert!(first_sent(&sent).0 <= 500);
    assert_eq!(
        decode(frame).header.protocol_version,
        ProtocolVersion::V2012
    );
    assert_eq!(identification(frame).entity_id, EID);
}

/// REQ 8.DoIP-051: each identification answer waits its own random
/// `A_DoIP_Announce_Wait`, so the answers of many entities to one broadcast do not all
/// arrive together.
#[test]
fn identification_answers_are_spread_by_the_announce_wait() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    let waits: Vec<u64> = (0..4)
        .map(|_| first_sent(&ask(&mut entity, &udp, &identification_request())).0)
        .collect();

    assert!(waits.iter().all(|wait| *wait <= 500), "{waits:?}");
    assert!(not_all_equal(&waits), "{waits:?}");
}

/// REQ 7.DoIP-156: the default protocol version `0xFF` is taken on an identification
/// request, and the answer is in this edition's, `0x03`.
#[test]
fn the_default_protocol_version_is_taken_on_identification_requests() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity().with_vin(vin()));

    for request in [
        identification_request(),
        raw(0x0002, &EID),
        raw(0x0003, &VIN),
    ] {
        let sent = ask(&mut entity, &udp, &versioned(request, 0xFF));
        assert_eq!(
            decode(answer(&sent)).header.protocol_version,
            ProtocolVersion::V2019
        );
    }
}

/// REQ 8.DoIP-047 and 053, Figure 13: a request with the entity's EID is answered, and
/// one with another's gets no answer at all.
#[test]
fn a_request_with_an_eid_is_answered_only_by_that_entity() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    let sent = ask(&mut entity, &udp, &raw(0x0002, &EID));
    assert_eq!(identification(answer(&sent)).entity_id, EID);

    assert_eq!(ask(&mut entity, &udp, &raw(0x0002, &[0xAA; 6])), []);
}

/// REQ 8.DoIP-048 and 052, Figure 13: a request with the entity's programmed VIN is
/// answered, one with another VIN is not, and while no VIN is programmed none is.
#[test]
fn a_request_with_a_vin_is_answered_only_with_the_programmed_vin() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity().with_vin(vin()));

    let sent = ask(&mut entity, &udp, &raw(0x0003, &VIN));
    assert_eq!(identification(answer(&sent)).vin, VIN);
    assert_eq!(
        ask(&mut entity, &udp, &raw(0x0003, b"1HGCM82633A004352")),
        []
    );

    restart_clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut unprogrammed = announced(&stack, &udp, identity());
    assert_eq!(ask(&mut unprogrammed, &udp, &raw(0x0003, &[0x00; 17])), []);
}

/// Figure 13: a request for another entity's EID or VIN is not this entity's to answer,
/// and the silence does not stop it answering the next request.
#[test]
fn after_a_request_for_another_entity_the_next_is_answered() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity().with_vin(vin()));

    let another_eid = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    let with_eid = versioned(raw(0x0002, &another_eid), 0x02);
    let with_vin = versioned(raw(0x0003, b"OTHER000000000001"), 0x02);
    assert_eq!(ask(&mut entity, &udp, &with_eid), []);
    assert_eq!(ask(&mut entity, &udp, &with_vin), []);

    let sent = ask(
        &mut entity,
        &udp,
        &versioned(identification_request(), 0x02),
    );
    assert_eq!(identification(answer(&sent)).entity_id, EID);
}

// --- power mode and entity status ------------------------------------------------------

/// REQ 8.DoIP-116 to 118: a power mode request is answered at once with the identity's
/// power mode.
#[test]
fn a_power_mode_request_is_answered_at_once() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    let sent = ask(&mut entity, &udp, &raw(0x4003, &[]));

    assert_eq!(first_sent(&sent).0, 0);
    assert_eq!(
        decode(answer(&sent)).payload,
        Payload::PowerModeInfoResponse(DiagnosticPowerModeCode::Ready)
    );
}

/// REQ 8.DoIP-119 to 121, Table 11: entity status is answered at once: a node, its
/// `MCTS`, the connection slots holding a socket, routing activated or not, and the
/// largest payload it takes.
#[test]
fn entity_status_counts_the_open_sockets() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());
    let status = |open| {
        Payload::EntityStatusResponse(EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: 1,
            open_tcp_sockets: open,
            max_data_size: Some(MDS),
        })
    };

    let sent = ask(&mut entity, &udp, &raw(0x4001, &[]));
    assert_eq!(first_sent(&sent).0, 0);
    assert_eq!(decode(answer(&sent)).payload, status(0));

    let _peer = stack.dial();
    settle(&mut entity);
    let sent = ask(&mut entity, &udp, &raw(0x4001, &[]));
    assert_eq!(decode(answer(&sent)).payload, status(1));
}

// --- the generic header handler on UDP (Figure 16) ---------------------------------------

/// REQ 7.DoIP-041: a wrong synchronisation pattern, or a protocol version the entity does
/// not take, `0xFF` on anything but an identification request among them, is answered
/// NACK `0x00`, and the port keeps listening.
#[test]
fn a_bad_pattern_is_answered_0x00() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    let mut bad_inverse = raw(0x4003, &[]);
    bad_inverse.splice(1..2, [0x00]);
    for datagram in [
        bad_inverse,
        versioned(raw(0x4003, &[]), 0xFF),
        versioned(raw(0x4003, &[]), 0x01),
    ] {
        let sent = ask(&mut entity, &udp, &datagram);
        assert_eq!(nack(&sent), NackCode::IncorrectPatternFormat);
    }
    assert_eq!(ask(&mut entity, &udp, &raw(0x4003, &[])).len(), 1);
}

/// REQ 7.DoIP-042: a payload type the entity does not take on UDP, a `TCP_DATA` one or a
/// reserved one, is answered NACK `0x01`.
#[test]
fn a_payload_type_not_taken_on_udp_is_answered_0x01() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    for datagram in [
        raw(0x0005, &[0x0E, 0x00, 0, 0, 0, 0, 0]),
        raw(0x0007, &[]),
        raw(0x8001, &[0x0E, 0x00, 0x00, 0x01, 0x3E]),
        raw(0x0009, &[]),
        raw(0xF000, &[]),
    ] {
        let sent = ask(&mut entity, &udp, &datagram);
        assert_eq!(nack(&sent), NackCode::UnknownPayloadType);
    }
}

/// REQ 7.DoIP-039, Figure 16: a header NACK, an announcement, the entity's own included,
/// and the responses a tester receives are discarded without an answer.
#[test]
fn what_testers_receive_is_discarded_silently() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = sensor(&stack, &udp, identity());
    let own = run_for(&mut entity, &udp, 500).remove(0).2;

    for datagram in [
        raw(0x0000, &[0x01]),
        versioned(raw(0x0000, &[0x00]), 0x01),
        own,
        raw(0x4002, &[0x01, 0x01, 0x00]),
        versioned(raw(0x4004, &[0x01]), 0x04),
        raw(0x4004, &[0x01]),
    ] {
        udp.deliver(tester_at(), &datagram);
        let sent = run_for(&mut entity, &udp, 1);
        assert!(sent.iter().all(|(_, to, _)| *to == broadcast()), "{sent:?}");
    }
}

/// REQ 7.DoIP-043: a payload longer than the max data size is answered NACK `0x02`,
/// before its length is held to its type's.
#[test]
fn a_payload_over_the_max_data_size_is_answered_0x02() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());
    let mut datagram = identification_request();
    datagram.splice(4..8, (MDS + 1).to_be_bytes());

    assert_eq!(
        nack(&ask(&mut entity, &udp, &datagram)),
        NackCode::MessageTooLarge
    );
}

/// REQ 7.DoIP-045: a payload length wrong for its type, or a datagram longer or shorter
/// than its header says, is answered NACK `0x04`, and the port keeps listening.
#[test]
fn a_wrong_length_is_answered_0x04() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity().with_vin(vin()));
    let mut trailing = raw(0x4003, &[]);
    trailing.push(0x00);
    let mut truncated = raw(0x0003, &VIN);
    truncated.pop();
    let mut oversized = raw(0x0003, &VIN);
    oversized.extend([0u8; 20]);

    for datagram in [
        raw(0x0001, &[0x00]),
        raw(0x0002, &EID[..5]),
        raw(0x4001, &[0x00]),
        trailing,
        truncated,
        oversized,
    ] {
        let sent = ask(&mut entity, &udp, &datagram);
        assert_eq!(
            nack(&sent),
            NackCode::InvalidPayloadLength,
            "{datagram:02x?}"
        );
    }
    assert_eq!(ask(&mut entity, &udp, &raw(0x4003, &[])).len(), 1);
}

/// ARCHITECTURE §2.5.3: a NACK raised once the request's protocol version is known is
/// sent in it, as the other answers are, so a tester of the 2012 edition can read it.
#[test]
fn a_nack_carries_the_requests_protocol_version() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    for datagram in [
        versioned(raw(0x4001, &[0x00]), 0x02),
        versioned(raw(0x0005, &[]), 0x02),
    ] {
        let sent = ask(&mut entity, &udp, &datagram);
        assert_eq!(
            decode(answer(&sent)).header.protocol_version,
            ProtocolVersion::V2012,
            "{datagram:02x?}"
        );
    }
}

/// REQ 7.DoIP-031: a datagram from a broadcast or multicast address is ignored; one too
/// short for a header has nothing to answer; and neither stops the next being answered.
#[test]
fn a_datagram_with_nothing_to_answer_is_dropped() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    udp.deliver(([255, 255, 255, 255], 50_000).into(), &raw(0x4003, &[]));
    udp.deliver(([224, 0, 0, 1], 50_000).into(), &raw(0x4003, &[]));
    assert_eq!(run_for(&mut entity, &udp, 1), []);
    assert_eq!(ask(&mut entity, &udp, &[0x03, 0xFC, 0x40]), []);
    assert_eq!(ask(&mut entity, &udp, &raw(0x4003, &[])).len(), 1);
}

/// REQ 7.DoIP-031 and 042, Figure 16: a truncated datagram gets nothing, an alive check
/// request, which is not taken on UDP, gets NACK `0x01`, and neither stops the entity
/// answering the identification request after them.
#[test]
fn a_datagram_the_entity_cannot_serve_does_not_stop_the_next() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    assert_eq!(ask(&mut entity, &udp, &[0x02, 0xFD, 0x00]), []);
    let sent = ask(&mut entity, &udp, &versioned(raw(0x0007, &[]), 0x02));
    assert_eq!(nack(&sent), NackCode::UnknownPayloadType);

    let sent = ask(
        &mut entity,
        &udp,
        &versioned(identification_request(), 0x02),
    );
    assert_eq!(identification(answer(&sent)).entity_id, EID);
}

// --- what the socket does ----------------------------------------------------------------

/// Answers the entity cannot hold are dropped: four testers' wait their
/// `A_DoIP_Announce_Wait`, and a fifth's arriving meanwhile is not answered.
#[test]
fn requests_beyond_the_pending_answers_are_dropped() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());

    for port in 50_001..=50_005 {
        udp.deliver(([192, 168, 0, 2], port).into(), &identification_request());
    }
    let sent = run_for(&mut entity, &udp, 600);
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert!(udp.all_received());
}

/// A tester repeating a request it is already owed an answer to is answered once, and
/// holds no more of the queue: another's power mode request is answered at once
/// (REQ 8.DoIP-118).
#[test]
fn a_repeated_request_is_answered_once_and_crowds_out_no_one() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());
    let other: SocketAddr = ([192, 168, 0, 3], 50_000).into();

    for _ in 0..5 {
        udp.deliver(tester_at(), &identification_request());
    }
    udp.deliver(other, &raw(0x4003, &[]));
    let sent = run_for(&mut entity, &udp, 600);

    let to = |address| sent.iter().filter(|(_, to, _)| *to == address).count();
    assert_eq!((to(tester_at()), to(other)), (1, 1), "{sent:?}");
    assert_eq!(
        sent.iter()
            .find(|(_, to, _)| *to == other)
            .map(|sent| sent.0),
        Some(0)
    );
}

/// A failed receive or send is told to the identity, and the socket is left alone for
/// 500 ms rather than retried at once; `next_event` neither fails nor spins, and the
/// `TCP_DATA` side carries on.
#[test]
fn a_failing_socket_rests_and_recovers() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let failures = RefCell::new(Vec::new());
    let mut entity = announced(&stack, &udp, Telling::new(&failures));
    assert_eq!(*failures.borrow(), []);

    udp.fail_receives();
    settle(&mut entity);
    assert_eq!(*failures.borrow(), [ErrorKind::Other]);
    let _peer = stack.dial();
    settle(&mut entity);

    udp.heal();
    udp.deliver(tester_at(), &raw(0x4003, &[]));
    assert_eq!(run_for(&mut entity, &udp, 499), []);
    assert_eq!(run_for(&mut entity, &udp, 1).len(), 1);
}

/// A send that fails is given up rather than retried, so an announcement the host cannot
/// broadcast does not hold back the answers after it.
#[test]
fn a_failed_send_is_given_up() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let failures = RefCell::new(Vec::new());
    let mut entity = sensor(&stack, &udp, Telling::new(&failures));
    udp.fail_sends();
    run_for(&mut entity, &udp, 500);
    assert_eq!(*failures.borrow(), [ErrorKind::Other]);

    udp.heal();
    udp.deliver(tester_at(), &raw(0x4003, &[]));
    let sent = run_for(&mut entity, &udp, 1500);

    let to = |address| sent.iter().filter(|(_, to, _)| *to == address).count();
    assert_eq!((to(tester_at()), to(broadcast())), (1, 2), "{sent:?}");
    assert_eq!(
        first_sent(&sent).0,
        0,
        "a failed send rests nothing: {sent:?}"
    );
}

/// A send that does not complete holds back no `TCP_DATA` socket: a tester activates
/// routing meanwhile.
#[test]
fn a_stalled_send_holds_up_nothing_else() {
    let _clock = clock();
    let (stack, udp) = (MockStack::new(4096), MockUdp::new());
    let mut entity = announced(&stack, &udp, identity());
    udp.stall_sends();
    udp.deliver(tester_at(), &raw(0x4003, &[]));
    settle(&mut entity);

    let peer = stack.dial();
    peer.send(&raw(0x0005, &[0x0E, 0x00, 0x00, 0, 0, 0, 0]));
    settle(&mut entity);
    assert!(peer.take_written().starts_with(&[0x03, 0xFC, 0x00, 0x06]));

    udp.resume_sends();
    settle(&mut entity);
    assert_eq!(udp.take_sent().len(), 1);
}
