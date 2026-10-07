//! `Entity` against a scripted acceptor: ISO 13400-2:2019's header handler (Figure 16),
//! diagnostic message handler (Figure 17), routing activation and socket handlers
//! (Figures 22, 26 to 28), the `TCP_DATA` timers of Table 12, and the
//! `DiagnosticEntity` contract.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

mod support;

use std::pin::pin;

use embassy_time::Duration;
use simple_doip::entity::{Entity, EntityAddress, Error};
use simple_doip::service::{
    ConnectionId, DiagnosticEntity, DoIpResult, EntityConfig, EntityEvent,
};
use simple_doip::{LogicalAddress, TaType};
use support::mock_stack::{
    ENTITY, MockPeer, MockStack, TESTER, ack, activation_response_for, advance,
    alive_check_request, alive_check_response, clock, diagnostic, header_nack, nack, raw,
    until_stalled,
};

const FUNCTIONAL: LogicalAddress = LogicalAddress(0xE400);
const OTHER: LogicalAddress = LogicalAddress(0x0E80);

const ACTIVATED: u8 = 0x10;

type OneSocket<'a> = Entity<'a, MockStack, 1, 4096, 2>;
type TwoSockets<'a> = Entity<'a, MockStack, 2, 4096, 2>;

fn address() -> EntityAddress {
    EntityAddress::new(ENTITY, FUNCTIONAL).unwrap()
}

fn two_testers() -> EntityConfig<2> {
    EntityConfig::new(
        [TESTER, OTHER].map(|sa| simple_doip::service::TesterAddress::new(sa).unwrap()),
    )
}

/// An event with its borrowed data copied out.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ev {
    Indication {
        connection: usize,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: Vec<u8>,
    },
    Truncated {
        connection: usize,
        pdu: Vec<u8>,
        length: usize,
    },
    Confirm {
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        result: DoIpResult,
    },
    Closed(usize),
    Deadline,
}

fn owned(event: EntityEvent<'_>) -> Ev {
    match event {
        EntityEvent::Indication {
            connection,
            sa,
            ta,
            ta_type,
            pdu,
        } => Ev::Indication {
            connection: connection.index(),
            sa,
            ta,
            ta_type,
            pdu: pdu.to_vec(),
        },
        EntityEvent::IndicationTruncated {
            connection,
            pdu,
            length,
            ..
        } => Ev::Truncated {
            connection: connection.index(),
            pdu: pdu.to_vec(),
            length,
        },
        EntityEvent::Confirm {
            sa,
            ta,
            ta_type,
            result,
        } => Ev::Confirm {
            sa,
            ta,
            ta_type,
            result,
        },
        EntityEvent::Closed { connection } => Ev::Closed(connection.index()),
        EntityEvent::Deadline => Ev::Deadline,
    }
}

/// Runs `next_event` into a buffer of `len` bytes until it returns, or until it waits
/// on something only the test can give, when it is dropped.
fn step_into<E: DiagnosticEntity>(
    entity: &mut E,
    len: usize,
    deadline_ms: Option<u32>,
) -> Option<Ev> {
    let mut buf = vec![0u8; len];
    let future = pin!(entity.next_event(&mut buf, deadline_ms));
    until_stalled(future).map(|event| owned(event.unwrap()))
}

fn step<E: DiagnosticEntity>(entity: &mut E) -> Option<Ev> {
    step_into(entity, 64, None)
}

/// Every event `next_event` returns before it waits.
fn events<E: DiagnosticEntity>(entity: &mut E) -> Vec<Ev> {
    std::iter::from_fn(|| step(entity)).collect()
}

fn activation_from(sa: LogicalAddress, activation_type: u8) -> Vec<u8> {
    let [high, low] = sa.0.to_be_bytes();
    raw(0x0005, &[high, low, activation_type, 0, 0, 0, 0])
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// A tester that connected and activated routing for `sa`.
fn activated<E: DiagnosticEntity>(
    stack: &MockStack,
    entity: &mut E,
    sa: LogicalAddress,
) -> MockPeer {
    let peer = stack.dial();
    peer.send(&activation_from(sa, 0));
    assert_eq!(events(entity), []);
    assert_eq!(peer.take_written(), activation_response_for(sa, ACTIVATED));
    peer
}

fn request<E: DiagnosticEntity>(
    entity: &mut E,
    ta: LogicalAddress,
    pdu: &[u8],
) -> Result<(), E::Error> {
    let future = pin!(entity.request(ENTITY, ta, TaType::Physical, pdu));
    until_stalled(future).expect("the request waits on nothing here")
}

// --- accepting and the timers of Table 12 ------------------------------------------------

/// REQ 3.DoIP-084 and 086: a socket with no routing activation is closed when
/// `T_TCP_Initial_Inactivity`, 2 s, elapses, and not before. No event names it.
#[test]
fn an_unactivated_socket_is_closed_after_t_tcp_initial_inactivity() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);

    advance(ms(1999));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());

    advance(ms(1));
    assert_eq!(events(&mut entity), []);
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-085: a valid routing activation request stops the initial inactivity
/// timer.
#[test]
fn routing_activation_stops_the_initial_inactivity_timer() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    advance(ms(10_000));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());
}

/// REQ 3.DoIP-080 and 082: a registered socket is closed once
/// `T_TCP_General_Inactivity`, 5 min, passes with no data either way, and the close is
/// reported for a connection an event named.
#[test]
fn a_registered_socket_is_closed_after_t_tcp_general_inactivity() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    assert_eq!(events(&mut entity), []);

    advance(ms(299_999));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());

    advance(ms(1));
    assert_eq!(events(&mut entity), [Ev::Closed(0)]);
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-124 and 080: an unsolicited alive check response keeps an idle
/// connection open.
#[test]
fn an_alive_check_response_keeps_an_idle_connection_open() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    advance(ms(200_000));
    peer.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);
    advance(ms(200_000));
    assert_eq!(events(&mut entity), []);

    assert!(!peer.is_shut());
    assert_eq!(peer.take_written(), []);
}

/// A connection beyond the `MCTS + 1` sockets the entity holds is accepted and dropped.
#[test]
fn surplus_connections_are_accepted_and_dropped() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let first = stack.dial();
    let reserve = stack.dial();
    let surplus = stack.dial();

    assert_eq!(events(&mut entity), []);

    assert!(surplus.is_dropped());
    assert!(!first.is_shut());
    assert!(!reserve.is_shut());
}

/// A caller's deadline that has already passed returns at once, but only after what is
/// owed: here, a request's confirm.
#[test]
fn a_past_deadline_returns_deadline_after_what_is_owed_and_without_waiting() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    advance(ms(10_000));
    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();

    assert_eq!(
        step_into(&mut entity, 64, Some(9_000)),
        Some(Ev::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::NoSocket
        })
    );
    assert_eq!(step_into(&mut entity, 64, Some(9_000)), Some(Ev::Deadline));
}

/// A caller's deadline ahead is waited for.
#[test]
fn a_deadline_ahead_returns_deadline_when_it_passes() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());

    assert_eq!(step_into(&mut entity, 64, Some(100)), None);
    advance(ms(100));
    assert_eq!(step_into(&mut entity, 64, Some(100)), Some(Ev::Deadline));
}

// --- Figure 16, the generic header handler -----------------------------------------------

/// REQ 7.DoIP-041: a header whose protocol version and its inverse do not match is
/// answered with NACK code 0x00, and the socket closed.
#[test]
fn a_bad_sync_pattern_is_nacked_0x00_and_the_socket_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&[0x03, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07]);

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), header_nack(0x00));
    assert!(peer.is_closed());
}

/// REQ 7.DoIP-042: a payload type the entity does not take on `TCP_DATA` is answered
/// with NACK code 0x01 and discarded; the connection goes on.
#[test]
fn an_unknown_payload_type_is_nacked_0x01_and_discarded() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&raw(0x4001, &[0xAA, 0xBB]));
    peer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);

    let mut expected = header_nack(0x01);
    expected.extend(activation_response_for(TESTER, ACTIVATED));
    assert_eq!(peer.take_written(), expected);
    assert!(!peer.is_shut());
}

/// REQ 7.DoIP-043: a payload longer than the entity's buffer is answered with NACK code
/// 0x02, read and dropped without being buffered, and the next frame handled.
#[test]
fn an_oversize_payload_is_nacked_0x02_discarded_and_the_next_frame_handled() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    let mut big = vec![0x0E, 0x00, 0x00, 0x01];
    big.extend(vec![0x55; 5000]);
    peer.send(&raw(0x8001, &big));
    peer.send(&diagnostic(TESTER, ENTITY, &[0x10, 0x03]));

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x10, 0x03]
        }]
    );
    let mut expected = header_nack(0x02);
    expected.extend(ack(ENTITY, TESTER));
    assert_eq!(peer.take_written(), expected);
}

/// REQ 7.DoIP-045: a payload length wrong for its payload type is answered with NACK
/// code 0x04, and the socket closed.
#[test]
fn a_wrong_payload_length_is_nacked_0x04_and_the_socket_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&raw(0x0005, &[0x0E, 0x00, 0x00, 0x00, 0x00]));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), header_nack(0x04));
    assert!(peer.is_closed());
}

/// REQ 7.DoIP-039: a generic header NACK the entity receives is ignored.
#[test]
fn a_received_header_nack_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&header_nack(0x00));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), []);
    assert!(!peer.is_shut());
}

// --- Figure 22, the routing activation handler -------------------------------------------

/// REQ 3.DoIP-062: the first routing activation on an entity is accepted with code 0x10.
#[test]
fn the_first_activation_is_accepted_0x10() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    assert!(!peer.is_shut());
}

/// REQ 3.DoIP-059: a tester whose source address the entity's configuration does not
/// accept is refused with code 0x00, and its socket closed.
#[test]
fn a_tester_outside_the_entity_config_is_refused_0x00_and_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Entity::<_, 1, 4096>::new(&stack, address(), EntityConfig::default());
    let peer = stack.dial();
    peer.send(&activation_from(OTHER, 0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), activation_response_for(OTHER, 0x00));
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-151: an activation type the entity does not support is refused with code
/// 0x06, and the socket closed.
#[test]
fn an_unsupported_activation_type_is_refused_0x06_and_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&activation_from(TESTER, 0xE0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), activation_response_for(TESTER, 0x06));
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-131: a diagnostic message sent right behind the routing activation
/// request, before its response, is read only once the socket is registered.
#[test]
fn a_frame_pipelined_behind_the_activation_is_handled_after_registration() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    let mut both = activation_from(TESTER, 0);
    both.extend(diagnostic(TESTER, ENTITY, &[0x22, 0xF1, 0x90]));
    peer.send(&both);

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x22, 0xF1, 0x90]
        }]
    );
    let mut expected = activation_response_for(TESTER, ACTIVATED);
    expected.extend(ack(ENTITY, TESTER));
    assert_eq!(peer.take_written(), expected);
}

// --- Figures 26 to 28, the socket handler ------------------------------------------------

/// REQ 3.DoIP-089: the source address already registered on the socket is accepted again.
#[test]
fn the_same_sa_on_the_same_socket_is_accepted_again() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(
        peer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    assert!(!peer.is_shut());
}

/// REQ 3.DoIP-106 and 149: a different source address on a registered socket is refused
/// with code 0x02, and the socket closed.
#[test]
fn a_different_sa_on_the_registered_socket_is_refused_0x02_and_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&activation_from(OTHER, 0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), activation_response_for(OTHER, 0x02));
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-091, 093 and 150: the source address registered on another socket that
/// answers its alive check keeps it; the newcomer is refused with code 0x03.
#[test]
fn the_sa_on_another_socket_that_answers_keeps_it_0x03() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());
    assert_eq!(newcomer.take_written(), []);

    holder.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);

    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, 0x03)
    );
    assert!(newcomer.is_closed());
    assert!(!holder.is_shut());
}

/// REQ 3.DoIP-091 and 092: the source address registered on another socket that stays
/// silent for `T_TCP_Alive_Check`, 500 ms, moves to the newcomer; the silent socket is
/// aborted and its close reported.
#[test]
fn the_sa_on_another_socket_that_is_silent_moves_after_t_tcp_alive_check() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    holder.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), ack(ENTITY, TESTER));
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());

    advance(ms(499));
    assert_eq!(events(&mut entity), []);
    assert_eq!(newcomer.take_written(), []);

    advance(ms(1));
    assert_eq!(events(&mut entity), [Ev::Closed(0)]);
    assert!(holder.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );

    newcomer.send(&diagnostic(TESTER, ENTITY, &[0x10, 0x01]));
    assert!(matches!(
        step(&mut entity),
        Some(Ev::Indication { connection: 0, .. })
    ));
}

/// REQ 3.DoIP-094 and 096: with every socket registered, a new tester's activation
/// checks them all; when all answer, it is refused with code 0x01.
#[test]
fn a_full_table_that_answers_refuses_0x01() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());

    holder.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);

    assert_eq!(
        newcomer.take_written(),
        activation_response_for(OTHER, 0x01)
    );
    assert!(newcomer.is_closed());
    assert!(!holder.is_shut());
}

/// REQ 3.DoIP-094 and 095: with every socket registered and none answering its alive
/// check within `T_TCP_Alive_Check`, the silent ones are aborted and the new tester
/// assigned.
#[test]
fn a_full_table_that_is_silent_is_freed_and_the_sa_assigned() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);

    advance(ms(500));
    assert_eq!(events(&mut entity), []);

    assert!(holder.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(OTHER, ACTIVATED)
    );
    assert!(!newcomer.is_shut());
}

/// REQ 3.DoIP-134: alive checks go to registered sockets only, never to one that has not
/// activated routing.
#[test]
fn only_registered_sockets_are_alive_checked() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let idle = stack.dial();
    let newcomer = stack.dial();
    assert_eq!(events(&mut entity), []);
    newcomer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(holder.take_written(), alive_check_request());
    assert_eq!(idle.take_written(), []);
}

/// The tester whose activation is being arbitrated leaving ends the arbitration: nothing
/// is answered, and the socket it challenged is left alone.
#[test]
fn the_arbitrating_tester_leaving_ends_the_arbitration() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());

    newcomer.eof();
    assert_eq!(events(&mut entity), []);
    advance(ms(1000));
    assert_eq!(events(&mut entity), []);

    assert!(!holder.is_shut());
    assert_eq!(newcomer.take_written(), []);
}

/// A tester on the reserve socket that activates routing while the one connection slot
/// holds a socket that has not moves into that slot, and the other into the reserve.
#[test]
fn a_reserve_activation_exchanges_with_an_initialized_slot() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let idle = stack.dial();
    let active = stack.dial();
    assert_eq!(events(&mut entity), []);
    active.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);
    assert_eq!(
        active.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );

    active.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    assert!(matches!(
        step(&mut entity),
        Some(Ev::Indication { connection: 0, .. })
    ));
    idle.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(
        active.take_written(),
        [ack(ENTITY, TESTER), alive_check_request()].concat()
    );
    active.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);
    assert_eq!(idle.take_written(), activation_response_for(OTHER, 0x01));
}

// --- Figure 17, diagnostic messages ------------------------------------------------------

/// REQ 7.DoIP-067: a diagnostic message to the entity is indicated and acknowledged.
#[test]
fn a_diagnostic_message_is_indicated_and_acknowledged() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x22, 0xF1, 0x90]));

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x22, 0xF1, 0x90]
        }]
    );
    assert_eq!(peer.take_written(), ack(ENTITY, TESTER));
}

/// The entity answers its one functional address as well as its physical one, and no
/// other functional address.
#[test]
fn the_entity_answers_its_functional_address_and_no_other() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, FUNCTIONAL, &[0x3E, 0x80]));
    peer.send(&diagnostic(TESTER, LogicalAddress(0xE401), &[0x3E, 0x80]));

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: FUNCTIONAL,
            ta_type: TaType::Functional,
            pdu: vec![0x3E, 0x80]
        }]
    );
    let mut expected = ack(FUNCTIONAL, TESTER);
    expected.extend(nack(LogicalAddress(0xE401), TESTER, 0x03));
    assert_eq!(peer.take_written(), expected);
}

/// REQ 7.DoIP-070: a diagnostic message from a source address not registered on its
/// socket is refused with code 0x02, and the socket closed.
#[test]
fn a_diagnostic_message_from_an_unregistered_sa_is_nacked_0x02_and_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(OTHER, ENTITY, &[0x3E, 0x00]));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), nack(ENTITY, OTHER, 0x02));
    assert!(peer.is_closed());
}

/// REQ 7.DoIP-070, and Figure 17 over REQ 3.DoIP-131: a diagnostic message before
/// routing activation is refused with code 0x02, and the socket closed.
#[test]
fn a_diagnostic_message_before_activation_is_nacked_0x02_and_closed() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), nack(ENTITY, TESTER, 0x02));
    assert!(peer.is_closed());
}

/// REQ 7.DoIP-071 and 074: a diagnostic message to a target the entity does not answer
/// is refused with code 0x03 and discarded; the connection goes on.
#[test]
fn a_diagnostic_message_to_an_unknown_ta_is_nacked_0x03_and_discarded() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    let unknown = LogicalAddress(0x0002);
    peer.send(&diagnostic(TESTER, unknown, &[0x3E, 0x00]));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), nack(unknown, TESTER, 0x03));
    assert!(!peer.is_shut());
}

/// A message longer than the caller's buffer is indicated truncated, with its length.
#[test]
fn a_message_longer_than_the_callers_buffer_is_indicated_truncated() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x2E, 0xF1, 0x90, 0x01, 0x02]));

    assert_eq!(
        step_into(&mut entity, 3, None),
        Some(Ev::Truncated {
            connection: 0,
            pdu: vec![0x2E, 0xF1, 0x90],
            length: 5
        })
    );
}

/// REQ 7.DoIP-042: a payload type the entity does not support is answered with NACK
/// code 0x01 and discarded on a registered connection too, whether ISO 13400-2:2019
/// Table 17 reserves it, ISO 14229-5:2022 REQ 7.16 gives it to the server's periodic
/// response, or it is left to the manufacturer.
#[test]
fn every_unsupported_payload_type_on_a_registered_connection_is_nacked_0x01() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    let unsupported = [
        0x0009, 0x4000, 0x4005, 0x8000, 0x8004, 0xEFFF, 0xF000, 0xFFFF,
    ];
    for payload_type in unsupported {
        peer.send(&raw(payload_type, &[0x01, 0x02]));
    }
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x3E, 0x00]
        }]
    );
    let mut expected = header_nack(0x01).repeat(unsupported.len());
    expected.extend(ack(ENTITY, TESTER));
    assert_eq!(peer.take_written(), expected);
    assert!(!peer.is_shut());
}

/// REQ 7.DoIP-042 before routing activation: a manufacturer-specific payload type is
/// answered with NACK code 0x01, not dropped unanswered.
#[test]
fn a_manufacturer_payload_type_before_activation_is_nacked_0x01() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&raw(0xF001, &[0x01, 0x02]));
    peer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);

    let mut expected = header_nack(0x01);
    expected.extend(activation_response_for(TESTER, ACTIVATED));
    assert_eq!(peer.take_written(), expected);
    assert!(!peer.is_shut());
}

// --- request, confirm and close ----------------------------------------------------------

/// A request is written to the connection that registered its target, and confirmed
/// once written.
#[test]
fn a_request_is_confirmed_once_written() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();

    assert_eq!(
        events(&mut entity),
        [Ev::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::Ok
        }]
    );
    assert_eq!(
        peer.take_written(),
        diagnostic(ENTITY, TESTER, &[0x7E, 0x00])
    );
}

/// A request whose target no connection registered is confirmed `NoSocket`.
#[test]
fn a_request_to_an_unregistered_ta_is_confirmed_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());

    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();

    assert_eq!(
        events(&mut entity),
        [Ev::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::NoSocket
        }]
    );
}

/// A request from a source address that is not the entity's sends nothing, and is
/// confirmed `UnknownSa`.
#[test]
fn a_request_from_another_sa_is_confirmed_unknown_sa() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    {
        let future = pin!(entity.request(FUNCTIONAL, TESTER, TaType::Physical, &[0x7E]));
        until_stalled(future).unwrap().unwrap();
    }

    assert_eq!(
        events(&mut entity),
        [Ev::Confirm {
            sa: FUNCTIONAL,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::UnknownSa
        }]
    );
    assert_eq!(peer.take_written(), []);
}

/// A request whose connection fails before it is written is confirmed `NoSocket`, then
/// the close reported.
#[test]
fn a_request_on_a_connection_that_closes_unwritten_is_confirmed_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    peer.fail_writes();

    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();

    assert_eq!(
        events(&mut entity),
        [
            Ev::Confirm {
                sa: ENTITY,
                ta: TESTER,
                ta_type: TaType::Physical,
                result: DoIpResult::NoSocket
            },
            Ev::Closed(0)
        ]
    );
    assert!(peer.is_aborted());
}

/// A request to a tester that has stopped reading waits for room in its queue, and is
/// confirmed `NoSocket` once the connection's general inactivity timer gives up on it.
#[test]
fn a_request_to_a_peer_that_stops_reading_waits_then_is_confirmed_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Entity::<_, 1, 64, 2>::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();
    request(&mut entity, TESTER, &[0x11; 40]).unwrap();

    {
        let mut waiting =
            pin!(entity.request(ENTITY, TESTER, TaType::Physical, &[0x22; 40]));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(ms(300_000));
        assert!(until_stalled(waiting.as_mut()).unwrap().is_ok());
    }

    let no_socket = Ev::Confirm {
        sa: ENTITY,
        ta: TESTER,
        ta_type: TaType::Physical,
        result: DoIpResult::NoSocket,
    };
    assert_eq!(events(&mut entity), [no_socket.clone(), no_socket]);
    assert!(peer.is_aborted());
}

/// A request is refused, with no confirm, where too many await theirs or the PDU does
/// not fit.
#[test]
fn a_request_that_cannot_be_held_is_refused() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Entity::<_, 1, 64, 2>::new(&stack, address(), two_testers());

    assert_eq!(
        request(&mut entity, TESTER, &[0; 60]),
        Err(Error::PduTooLarge { len: 60, max: 52 })
    );
    for _ in 0..4 {
        request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();
    }
    assert_eq!(
        request(&mut entity, TESTER, &[0x7E, 0x00]),
        Err(Error::RequestQueueFull)
    );
}

/// ISO 14229-5:2022 REQ 7.9 and 7.11: the caller's close writes what was requested on
/// the connection first, then closes it; no close is reported for it.
#[test]
fn close_writes_what_was_queued_then_closes() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x11, 0x01]));
    let Some(Ev::Indication { connection, .. }) = step(&mut entity) else {
        panic!("expected the indication");
    };
    request(&mut entity, TESTER, &[0x51, 0x01]).unwrap();

    {
        let closing =
            pin!(entity.close(ConnectionId::new(u8::try_from(connection).unwrap())));
        until_stalled(closing).unwrap().unwrap();
    }

    assert_eq!(
        peer.take_written(),
        [
            ack(ENTITY, TESTER),
            diagnostic(ENTITY, TESTER, &[0x51, 0x01])
        ]
        .concat()
    );
    assert!(peer.is_closed());
    assert_eq!(
        events(&mut entity),
        [Ev::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::Ok
        }]
    );
}

/// Closing a connection that has gone does nothing.
#[test]
fn closing_a_connection_that_is_not_there_does_nothing() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());

    for index in [0, 7] {
        let closing = pin!(entity.close(ConnectionId::new(index)));
        until_stalled(closing).unwrap().unwrap();
    }
    assert_eq!(events(&mut entity), []);
}

/// The `DiagnosticEntity` contract, driven as the layer above drives it: every
/// indication answered, every request confirmed exactly once, closes reported once.
#[test]
fn the_trait_contract_holds_for_an_echoing_caller() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let first = activated(&stack, &mut entity, TESTER);
    let second = activated(&stack, &mut entity, OTHER);
    first.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    second.send(&diagnostic(OTHER, ENTITY, &[0x10, 0x01]));

    let mut confirms = Vec::new();
    let mut indications = Vec::new();
    while let Some(event) = step(&mut entity) {
        match event {
            Ev::Indication {
                sa,
                pdu,
                connection,
                ..
            } => {
                indications.push((connection, sa));
                request(&mut entity, sa, &pdu).unwrap();
            }
            Ev::Confirm { ta, result, .. } => confirms.push((ta, result)),
            other => panic!("unexpected {other:?}"),
        }
    }

    indications.sort_by_key(|(connection, _)| *connection);
    assert_eq!(indications, [(0, TESTER), (1, OTHER)]);
    confirms.sort_by_key(|(ta, _)| ta.0);
    assert_eq!(
        confirms,
        [(TESTER, DoIpResult::Ok), (OTHER, DoIpResult::Ok)]
    );
    assert_eq!(
        first.take_written(),
        [
            ack(ENTITY, TESTER),
            diagnostic(ENTITY, TESTER, &[0x3E, 0x00])
        ]
        .concat()
    );
    assert_eq!(
        second.take_written(),
        [ack(ENTITY, OTHER), diagnostic(ENTITY, OTHER, &[0x10, 0x01])].concat()
    );

    second.eof();
    assert_eq!(events(&mut entity), [Ev::Closed(1)]);
    assert_eq!(events(&mut entity), []);
}
