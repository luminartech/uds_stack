//! The tester's discovery against a scripted UDP socket: vehicle identification
//! (ISO 13400-2:2019 7.4), diagnostic power mode (7.5) and entity status (7.6), each
//! waiting `A_DoIP_Ctrl` (Table 12).

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used, clippy::panic)]

mod support;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::pin;

use embassy_time::Duration;
use simple_doip::messages::{
    DiagnosticPowerModeCode, EntityStatusNodeType, EntityStatusResponse, NackCode,
};
use simple_doip::tester::discovery::{
    self, A_DOIP_CTRL, BROADCAST, DiscoveryError, Request,
};
use simple_doip::{EntityId, TCP_PORT, UDP_DISCOVERY_PORT, Vin};
use support::mock_stack::{
    MockError, MockUdp, advance, clock, raw, until_stalled, versioned,
};

const EID: [u8; 6] = [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF];
const OTHER_EID: [u8; 6] = [0x02, 0x00, 0x00, 0x12, 0x34, 0x56];
const VIN: [u8; 17] = *b"WVWZZZ1JZXW000001";

fn entity_at(last: u8) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last)),
        UDP_DISCOVERY_PORT,
    )
}

/// An identification response from the entity at `address` with `eid`.
fn identification(address: u16, eid: [u8; 6]) -> Vec<u8> {
    let mut payload = VIN.to_vec();
    payload.extend(address.to_be_bytes());
    payload.extend(eid);
    payload.extend([0x00; 6]);
    payload.push(0x00);
    raw(0x0004, &payload)
}

fn just_under_a_doip_ctrl() -> Duration {
    A_DOIP_CTRL.checked_sub(Duration::from_millis(1)).unwrap()
}

/// REQ 4.DoIP-136, 7.DoIP-156 and Table 12: an identification request goes out once, in
/// the default protocol version, and every distinct entity answering within `A_DoIP_Ctrl`
/// is found; a repeated answer, a frame of another type, and a datagram that is not a
/// frame are passed over.
#[test]
fn identify_finds_each_entity_that_answers_within_a_doip_ctrl() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut found = [None; 4];
    let kept = {
        let mut identifying = pin!(discovery::identify(
            &mut socket,
            BROADCAST,
            Request::All,
            &mut found
        ));
        assert!(until_stalled(identifying.as_mut()).is_none());
        assert_eq!(
            udp.take_sent(),
            [(BROADCAST, vec![0xFF, 0x00, 0x00, 0x01, 0, 0, 0, 0])]
        );
        udp.deliver(entity_at(10), &identification(0xE400, EID));
        udp.deliver(entity_at(10), &identification(0xE400, EID));
        udp.deliver(entity_at(11), &raw(0x4004, &[0x01]));
        udp.deliver(entity_at(12), &[0xDE, 0xAD]);
        udp.deliver(entity_at(11), &identification(0xE401, OTHER_EID));
        assert!(until_stalled(identifying.as_mut()).is_none());

        advance(just_under_a_doip_ctrl());
        assert!(until_stalled(identifying.as_mut()).is_none());
        advance(Duration::from_millis(1));
        until_stalled(identifying.as_mut())
    };

    assert_eq!(kept, Some(Ok(2)));

    let [Some(first), Some(second), None, None] = &found else {
        panic!("expected two entities, got {found:?}");
    };
    assert_eq!(first.address(), entity_at(10));
    assert_eq!(first.identification().entity_id, EID);
    assert_eq!(second.identification().logical_address.0, 0xE401);
    assert_eq!(
        second.tcp_address(),
        SocketAddr::new(entity_at(11).ip(), TCP_PORT)
    );
}

/// ISO 13400-2:2019 Tables 3 and 4: a directed request carries the EID or VIN it names,
/// in the default protocol version too.
#[test]
fn a_directed_request_carries_what_it_names() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut found = [None; 1];
    let to = entity_at(10);

    for (request, expected) in [
        (
            Request::Eid(EntityId::new(EID).unwrap()),
            versioned(raw(0x0002, &EID), 0xFF),
        ),
        (
            Request::Vin(Vin::new(VIN).unwrap()),
            versioned(raw(0x0003, &VIN), 0xFF),
        ),
    ] {
        let mut identifying =
            pin!(discovery::identify(&mut socket, to, request, &mut found));
        assert!(until_stalled(identifying.as_mut()).is_none());
        assert_eq!(udp.take_sent(), [(to, expected)]);
        advance(A_DOIP_CTRL);
        assert_eq!(until_stalled(identifying.as_mut()), Some(Ok(0)));
    }
}

/// Entities answering once `found` is full are not kept.
#[test]
fn identify_keeps_no_more_than_it_has_room_for() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut found = [None; 1];
    let kept = {
        let mut identifying = pin!(discovery::identify(
            &mut socket,
            BROADCAST,
            Request::All,
            &mut found
        ));
        assert!(until_stalled(identifying.as_mut()).is_none());
        udp.deliver(entity_at(10), &identification(0xE400, EID));
        udp.deliver(entity_at(11), &identification(0xE401, OTHER_EID));
        advance(A_DOIP_CTRL);
        until_stalled(identifying.as_mut())
    };

    assert_eq!(kept, Some(Ok(1)));
    let [Some(only)] = &found else {
        panic!("expected one entity, got {found:?}");
    };
    assert_eq!(only.identification().entity_id, EID);
}

/// REQ 8.DoIP-119 to 121, Table 11: the entity's status, its answer taken from it alone.
#[test]
fn entity_status_comes_from_the_entity_asked() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut asking = pin!(discovery::entity_status(&mut socket, entity_at(10)));

    assert!(until_stalled(asking.as_mut()).is_none());
    assert_eq!(udp.take_sent(), [(entity_at(10), raw(0x4001, &[]))]);
    udp.deliver(entity_at(11), &raw(0x4002, &[0x00, 4, 0, 0, 0, 0x10, 0x00]));
    udp.deliver(entity_at(10), &raw(0x4002, &[0x01, 1, 0, 0, 0, 0x0F, 0xF8]));

    assert_eq!(
        until_stalled(asking.as_mut()),
        Some(Ok(EntityStatusResponse {
            node_type: EntityStatusNodeType::DoIPNode,
            max_concurrent_tcp_sockets: 1,
            open_tcp_sockets: 0,
            max_data_size: Some(4088),
        }))
    );
}

/// REQ 8.DoIP-116 to 118: the vehicle's power mode.
#[test]
fn power_mode_comes_from_the_entity_asked() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut asking = pin!(discovery::power_mode(&mut socket, entity_at(10)));

    assert!(until_stalled(asking.as_mut()).is_none());
    assert_eq!(udp.take_sent(), [(entity_at(10), raw(0x4003, &[]))]);
    udp.deliver(entity_at(10), &raw(0x4004, &[0x01]));

    assert_eq!(
        until_stalled(asking.as_mut()),
        Some(Ok(DiagnosticPowerModeCode::Ready))
    );
}

/// Table 19: an entity that does not support the request refuses it with a header NACK.
#[test]
fn a_header_nack_is_a_refusal() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut asking = pin!(discovery::entity_status(&mut socket, entity_at(10)));

    assert!(until_stalled(asking.as_mut()).is_none());
    udp.deliver(entity_at(10), &raw(0x0000, &[0x01]));

    assert_eq!(
        until_stalled(asking.as_mut()),
        Some(Err(DiscoveryError::Refused(NackCode::UnknownPayloadType)))
    );
}

/// Table 12: nothing within `A_DoIP_Ctrl` is no answer.
#[test]
fn silence_for_a_doip_ctrl_is_no_answer() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();
    let mut asking = pin!(discovery::power_mode(&mut socket, entity_at(10)));

    assert!(until_stalled(asking.as_mut()).is_none());
    advance(just_under_a_doip_ctrl());
    assert!(until_stalled(asking.as_mut()).is_none());
    advance(Duration::from_millis(1));

    assert_eq!(
        until_stalled(asking.as_mut()),
        Some(Err(DiscoveryError::NoAnswer))
    );
}

/// A socket that fails is reported, on sending and on receiving.
#[test]
fn a_failing_socket_is_an_io_error() {
    let _clock = clock();
    let udp = MockUdp::new();
    let mut socket = udp.socket();

    udp.fail_sends();
    assert_eq!(
        until_stalled(pin!(discovery::power_mode(&mut socket, entity_at(10)))),
        Some(Err(DiscoveryError::Io(MockError)))
    );

    udp.heal();
    let mut found = [None; 1];
    let mut identifying = pin!(discovery::identify(
        &mut socket,
        BROADCAST,
        Request::All,
        &mut found
    ));
    assert!(until_stalled(identifying.as_mut()).is_none());
    udp.fail_receives();
    udp.deliver(entity_at(10), &identification(0xE400, EID));
    assert_eq!(
        until_stalled(identifying.as_mut()),
        Some(Err(DiscoveryError::Io(MockError)))
    );
}
