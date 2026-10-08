//! `Entity` against a scripted acceptor: ISO 13400-2:2019's header handler (Figure 16),
//! diagnostic message handler (Figure 17), routing activation and socket handlers
//! (Figures 22, 26 to 28), the `TCP_DATA` timers of Table 12, and the
//! `DiagnosticEntity` contract.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::pin::pin;

use embassy_time::Duration;
use simple_doip::entity::{AddressError, Entity, EntityAddress, Error};
use simple_doip::service::{
    ConnectionId, DiagnosticEntity, DoIpResult, EntityConfig, EntityEvent, Refusal,
    Timestamp,
};
use simple_doip::{LogicalAddress, TaType};
use support::mock_stack::{
    ENTITY, MockError, MockPeer, MockStack, TESTER, ack, activation_response_for, advance,
    alive_check_request, alive_check_response, clock, diagnostic, header_nack, nack,
    poll_times, raw, until_stalled,
};
use support::the_tester;

const FUNCTIONAL: LogicalAddress = LogicalAddress(0xE400);
const OTHER: LogicalAddress = LogicalAddress(0x0E80);

const ACTIVATED: u8 = 0x10;

type OneSocket<'a> = Entity<'a, MockStack, 1, 4096, 2>;

/// An entity whose 64-byte messages make its queue easy to fill.
type Small<'a> = Entity<'a, MockStack, 1, 64, 2>;
/// The longest PDU a [`Small`] entity sends.
const SMALL_PDU: usize = <Small<'static> as DiagnosticEntity>::MAX_PDU;
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
    deadline: Option<Timestamp>,
) -> Option<Ev> {
    let mut buf = vec![0u8; len];
    let future = pin!(entity.next_event(&mut buf, deadline));
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

/// `frame` with the protocol version of ISO 13400-2:2012.
fn in_2012(mut frame: Vec<u8>) -> Vec<u8> {
    frame.splice(..2, [0x02, 0xFD]);
    frame
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
) -> Result<(), Refusal> {
    let future = pin!(entity.request(ENTITY, ta, TaType::Physical, pdu));
    until_stalled(future).expect("the request waits on nothing here")
}

fn confirm(sa: LogicalAddress, ta_type: TaType, result: DoIpResult) -> Ev {
    Ev::Confirm {
        sa,
        ta: TESTER,
        ta_type,
        result,
    }
}

// --- accepting and the timers of Table 12 ------------------------------------------------

/// An entity's physical address is neither functional nor a tester's, and its functional
/// address is functional.
#[test]
fn an_entity_address_is_physical_then_functional() {
    let functional = LogicalAddress(0xE400);
    assert_eq!(
        EntityAddress::new(functional, functional),
        Err(AddressError::NotPhysical(functional))
    );
    assert_eq!(
        EntityAddress::new(TESTER, functional),
        Err(AddressError::NotPhysical(TESTER))
    );
    assert_eq!(
        EntityAddress::new(ENTITY, ENTITY),
        Err(AddressError::NotFunctional(ENTITY))
    );
    assert!(EntityAddress::new(ENTITY, functional).is_ok());
}

/// A failed accept is the caller's to handle: `next_event` returns it.
#[test]
fn a_failed_accept_is_returned() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    stack.fail_next_accept();

    let mut buf = [0u8; 64];
    assert!(matches!(
        until_stalled(pin!(entity.next_event(&mut buf, None))),
        Some(Err(Error::Accept(MockError)))
    ));
}

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

/// REQ 4.DoIP-002: the entity holds `MCTS + 1` sockets; a connection beyond them is
/// accepted and dropped.
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

/// The `DiagnosticEntity` contract: a caller's deadline that has already passed returns
/// at once, but only after what is owed: here, a request's confirm.
#[test]
fn a_past_deadline_returns_deadline_after_what_is_owed_and_without_waiting() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    advance(ms(10_000));
    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();

    assert_eq!(
        step_into(&mut entity, 64, Some(Timestamp(9_000))),
        Some(Ev::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::NoSocket
        })
    );
    assert_eq!(
        step_into(&mut entity, 64, Some(Timestamp(9_000))),
        Some(Ev::Deadline)
    );
}

/// The `DiagnosticEntity` contract: a caller's deadline ahead is waited for.
#[test]
fn a_deadline_ahead_returns_deadline_when_it_passes() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());

    assert_eq!(step_into(&mut entity, 64, Some(Timestamp(100))), None);
    advance(ms(100));
    assert_eq!(
        step_into(&mut entity, 64, Some(Timestamp(100))),
        Some(Ev::Deadline)
    );
}

/// REQ 3.DoIP-080: data the entity sends restarts `T_TCP_General_Inactivity` as data it
/// receives does.
#[test]
fn data_sent_restarts_t_tcp_general_inactivity() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    advance(ms(200_000));
    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();
    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    advance(ms(299_999));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());

    advance(ms(1));
    assert_eq!(events(&mut entity), []);
    assert!(peer.is_closed());
}

/// A `next_event` already waiting wakes when `T_TCP_Initial_Inactivity` elapses.
#[test]
fn a_waiting_next_event_wakes_for_t_tcp_initial_inactivity() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    let mut buf = [0u8; 64];
    let mut waiting = pin!(entity.next_event(&mut buf, None));
    assert!(until_stalled(waiting.as_mut()).is_none());

    advance(ms(2000));
    assert!(until_stalled(waiting.as_mut()).is_none());
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-085: a routing activation request that reached the socket before
/// `T_TCP_Initial_Inactivity` elapsed is handled, however late `next_event` is called.
#[test]
fn an_activation_that_arrived_in_time_is_handled_though_next_event_is_late() {
    let _clock = clock();
    let stack = MockStack::eager(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);
    advance(ms(1900));
    peer.send(&activation_from(TESTER, 0));
    advance(ms(200));

    assert_eq!(events(&mut entity), []);

    assert_eq!(
        peer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    assert!(!peer.is_shut());
}

/// A socket a passed deadline judges is read before the deadline is acted on, but no
/// further than its buffer's worth: a peer that keeps sending holds off neither the
/// deadline nor the caller's.
#[test]
fn a_judged_socket_that_keeps_sending_does_not_hold_off_the_deadlines() {
    let _clock = clock();
    let stack = MockStack::eager(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);
    advance(ms(2000));
    peer.send(&alive_check_response().repeat(20_000));

    let now = entity.now();
    assert_eq!(step_into(&mut entity, 64, Some(now)), Some(Ev::Deadline));
    assert!(!peer.all_read());

    assert_eq!(events(&mut entity), []);
    assert!(peer.is_closed());
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

/// REQ 7.DoIP-044: before routing activation a socket's frames must fit the reserve's
/// buffer. A longer payload, within the entity's maximum data size, exceeds only the
/// memory it has for that socket: it is answered with NACK code 0x03 and discarded, and
/// the next frame handled.
#[test]
fn a_payload_too_long_before_activation_is_nacked_0x03_and_discarded() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&diagnostic(TESTER, ENTITY, &[0x22; 21]));
    peer.send(&activation_from(TESTER, 0));

    assert_eq!(events(&mut entity), []);

    let mut expected = header_nack(0x03);
    expected.extend(activation_response_for(TESTER, ACTIVATED));
    assert_eq!(peer.take_written(), expected);
    assert!(!peer.is_shut());
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

/// REQ 7.DoIP-043 at its boundary: a frame of exactly the entity's buffer is taken, and
/// one a byte longer is answered with NACK code 0x02.
#[test]
fn a_frame_that_fills_the_buffer_is_taken_and_one_a_byte_longer_is_nacked_0x02() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    let fits = diagnostic(TESTER, ENTITY, &[0x22; 52]);
    assert_eq!(fits.len(), 64);

    peer.send(&fits);
    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x22; 52],
        }]
    );
    assert_eq!(peer.take_written(), ack(ENTITY, TESTER));

    peer.send(&diagnostic(TESTER, ENTITY, &[0x22; 53]));
    assert_eq!(events(&mut entity), []);
    assert_eq!(peer.take_written(), header_nack(0x02));
    assert!(!peer.is_shut());
}

/// An acknowledgement still queued when the response to its request is requested leaves
/// with it in one write, which Nagle's algorithm sends at once where it would hold back
/// a second write until the tester acknowledges the first.
#[test]
fn an_acknowledgement_and_its_response_leave_in_one_write() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.take_writes();
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));

    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    assert_eq!(peer.take_writes(), []);
    request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();
    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );

    let both = [
        ack(ENTITY, TESTER),
        diagnostic(ENTITY, TESTER, &[0x7E, 0x00]),
    ]
    .concat();
    assert_eq!(peer.take_writes(), [both.len()]);
    assert_eq!(peer.take_written(), both);
}

/// Table 16: a tester speaking ISO 13400-2:2012 is accepted, and answered in the version
/// it used.
#[test]
fn a_2012_tester_is_answered_in_2012() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&in_2012(activation_from(TESTER, 0)));

    assert_eq!(events(&mut entity), []);

    assert_eq!(
        peer.take_written(),
        in_2012(activation_response_for(TESTER, ACTIVATED))
    );
}

/// A header NACK carries the protocol version of the frame it refuses, as every other
/// answer does: here, a 2012 tester's first frame.
#[test]
fn a_header_nack_carries_the_refused_frames_version() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&in_2012(raw(0x4001, &[])));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), in_2012(header_nack(0x01)));
}

/// REQ 7.DoIP-045, Tables 46 and 28: a routing activation request is 7 or 11 bytes
/// and an alive check response 2; any other length is answered with NACK code 0x04.
#[test]
fn control_frame_lengths_are_checked_exactly() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let long = stack.dial();
    long.send(&raw(
        0x0005,
        &[0x0E, 0x00, 0, 0, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD],
    ));
    assert_eq!(events(&mut entity), []);
    assert_eq!(
        long.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );

    let odd = stack.dial();
    odd.send(&raw(0x0005, &[0x0E, 0x80, 0, 0, 0, 0, 0, 0]));
    assert_eq!(events(&mut entity), []);
    assert_eq!(odd.take_written(), header_nack(0x04));

    long.send(&raw(0x0008, &[0x0E, 0x00, 0x00]));
    assert_eq!(events(&mut entity), []);
    assert_eq!(long.take_written(), header_nack(0x04));
    assert!(long.is_closed());
}

/// A socket closing after a NACK whose writes cannot finish within the orderly close's
/// 2 s is aborted, and not before.
#[test]
fn an_orderly_close_that_cannot_write_is_aborted() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);
    peer.stall_writes();
    peer.send(&[0x03, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07]);
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());

    advance(ms(1999));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());

    advance(ms(1));
    assert_eq!(events(&mut entity), []);
    assert!(peer.is_aborted());
}

/// An orderly close whose write fails becomes an abort with `T_TCP_Alive_Check` of its
/// own, rather than what was left of the close's, before the socket is dropped.
#[test]
fn an_abort_after_a_failed_orderly_close_gets_its_own_time() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);
    peer.stall_writes();
    peer.stall_aborts();
    peer.send(&[0x03, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07]);
    assert_eq!(events(&mut entity), []);

    advance(ms(400));
    peer.fail_writes();
    assert_eq!(events(&mut entity), []);
    advance(ms(100));
    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_dropped());

    advance(ms(400));
    assert_eq!(events(&mut entity), []);
    assert!(peer.is_dropped());
}

/// A named socket dropped when its abort does not finish is reported `Closed` by the
/// same `next_event` that dropped it, not after some unrelated wake.
#[test]
fn a_socket_dropped_by_its_timer_is_reported_at_once() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    assert_eq!(events(&mut entity), []);
    peer.stall_closes();
    peer.stall_aborts();

    advance(ms(300_000));
    assert_eq!(step(&mut entity), None);
    advance(ms(2000));
    assert_eq!(step(&mut entity), None);
    assert!(!peer.is_dropped());
    advance(ms(500));
    assert_eq!(step(&mut entity), Some(Ev::Closed(0)));
    assert!(peer.is_dropped());
}

/// A socket closing after a NACK whose write fails is aborted.
#[test]
fn a_closing_socket_whose_write_fails_is_aborted() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    assert_eq!(events(&mut entity), []);
    peer.fail_writes();
    peer.send(&[0x03, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07]);

    assert_eq!(events(&mut entity), []);
    assert!(peer.is_aborted());
}

/// REQ 7.DoIP-044 on the reserve socket too: before activation, a frame within the
/// entity's maximum data size but too long for the reserve's buffer is refused with NACK
/// code 0x03, not 0x02.
#[test]
fn a_payload_too_long_for_the_reserve_before_activation_is_nacked_0x03() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let _holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&diagnostic(OTHER, ENTITY, &[0x22; 21]));

    assert_eq!(events(&mut entity), []);

    assert_eq!(newcomer.take_written(), header_nack(0x03));
    assert!(!newcomer.is_shut());
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
    let mut entity = Entity::<_, 1, 4096>::new(&stack, address(), the_tester());
    let peer = stack.dial();
    peer.send(&activation_from(OTHER, 0));

    assert_eq!(events(&mut entity), []);

    assert_eq!(peer.take_written(), activation_response_for(OTHER, 0x00));
    assert!(peer.is_closed());
}

/// REQ 3.DoIP-100 and Table 47: activation type 0x01, diagnostic communication required
/// by regulation, is mandatory, and accepted as the default is.
#[test]
fn the_regulation_activation_type_is_accepted_0x10() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = stack.dial();
    peer.send(&activation_from(TESTER, 0x01));

    assert_eq!(events(&mut entity), []);

    assert_eq!(
        peer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    assert!(!peer.is_shut());
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

/// Figure 26: the tester whose activation is being arbitrated leaving ends the
/// arbitration: nothing is answered, the socket it challenged is left alone, and the
/// next activation is arbitrated afresh.
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

    let next = stack.dial();
    next.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());
    holder.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);
    assert_eq!(next.take_written(), activation_response_for(TESTER, 0x03));
}

/// REQ 4.DoIP-002 and Figure 26: a tester on the reserve socket that activates routing
/// while the one connection slot holds a socket that has not moves into that slot, and
/// the other into the reserve.
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

/// REQ 3.DoIP-085: a routing activation request stops `T_TCP_Initial_Inactivity` even
/// while it waits on an alive check.
#[test]
fn an_activation_under_arbitration_is_not_closed_by_the_initial_timer() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    assert_eq!(events(&mut entity), []);
    advance(ms(1800));
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());

    advance(ms(300));
    assert_eq!(events(&mut entity), []);
    assert!(!newcomer.is_shut());

    advance(ms(200));
    assert_eq!(events(&mut entity), []);
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
}

/// REQ 3.DoIP-093: an alive check response that reached the socket within
/// `T_TCP_Alive_Check` keeps the holder's registration, however late `next_event` is
/// called.
#[test]
fn an_alive_check_response_that_arrived_in_time_counts_though_next_event_is_late() {
    let _clock = clock();
    let stack = MockStack::eager(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());
    advance(ms(100));
    holder.send(&alive_check_response());
    advance(ms(450));

    assert_eq!(events(&mut entity), []);

    assert!(!holder.is_shut());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, 0x03)
    );
}

/// REQ 3.DoIP-085: a routing activation request held behind another socket's
/// arbitration has still stopped its socket's `T_TCP_Initial_Inactivity`.
#[test]
fn an_activation_held_behind_an_arbitration_stops_the_initial_timer() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let late = stack.dial();
    assert_eq!(events(&mut entity), []);
    advance(ms(1600));
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());
    advance(ms(100));
    late.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);

    advance(ms(400));
    assert_eq!(events(&mut entity), []);

    assert!(holder.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    assert_eq!(
        late.take_written(),
        activation_response_for(OTHER, ACTIVATED)
    );
    assert!(!late.is_shut());
}

/// A `next_event` already waiting wakes when an arbitration's `T_TCP_Alive_Check`
/// elapses.
#[test]
fn a_waiting_next_event_wakes_for_the_alive_check_expiry() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    {
        let mut buf = [0u8; 64];
        let mut waiting = pin!(entity.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        assert_eq!(holder.take_written(), alive_check_request());

        advance(ms(500));
        assert!(until_stalled(waiting.as_mut()).is_none());
    }
    assert!(holder.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
}

/// REQ 3.DoIP-094 and 095 with two sockets: of the registered sockets, those, and only
/// those, that do not answer within `T_TCP_Alive_Check` are closed.
#[test]
fn a_full_table_closes_only_and_all_its_silent_sockets() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let third = LogicalAddress(0x0E01);
    let three = EntityConfig::new(
        [TESTER, OTHER, third]
            .map(|sa| simple_doip::service::TesterAddress::new(sa).unwrap()),
    );
    let mut entity = Entity::<_, 2, 4096, 3>::new(&stack, address(), three);
    let answers = activated(&stack, &mut entity, TESTER);
    let silent = activated(&stack, &mut entity, OTHER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(third, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(answers.take_written(), alive_check_request());
    assert_eq!(silent.take_written(), alive_check_request());
    answers.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);

    advance(ms(500));
    assert_eq!(events(&mut entity), []);
    assert!(silent.is_aborted());
    assert!(!answers.is_shut());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(third, ACTIVATED)
    );
}

/// Figure 26: a second activation arriving during an arbitration waits for it to end,
/// and is then arbitrated afresh.
#[test]
fn a_second_activation_waits_for_the_arbitration_in_progress() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let second = stack.dial();
    let first = stack.dial();
    assert_eq!(events(&mut entity), []);
    first.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());
    second.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);

    holder.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);
    assert_eq!(first.take_written(), activation_response_for(TESTER, 0x03));
    assert_eq!(holder.take_written(), alive_check_request());
}

/// REQ 3.DoIP-092: a holder that has stopped reading never receives its alive check
/// request, so does not answer, and is closed after `T_TCP_Alive_Check`; a response
/// filling its queue does not keep the request from being queued.
#[test]
fn a_holder_that_has_stopped_reading_is_silent() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    holder.stall_writes();
    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(newcomer.take_written(), []);

    advance(ms(500));
    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::NoSocket)]
    );
    assert!(holder.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
}

/// REQ 3.DoIP-093: an alive check response counts though the holder has stopped reading
/// and has not yet been sent the request.
#[test]
fn a_response_from_a_holder_not_yet_sent_the_request_counts() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    holder.stall_writes();
    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);

    holder.send(&alive_check_response());
    assert_eq!(events(&mut entity), []);

    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, 0x03)
    );
    assert!(!holder.is_shut());
}

/// The entity's own frames queue apart from responses: an alive check request asked for
/// while a response fills the queue goes out ahead of it, rather than waiting for it to
/// be written (REQ 3.DoIP-091).
#[test]
fn an_alive_check_request_is_not_held_behind_a_full_response() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    holder.stall_writes();
    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);

    holder.resume_writes();
    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    let mut expected = alive_check_request();
    expected.extend(diagnostic(ENTITY, TESTER, &[0x62; SMALL_PDU]));
    assert_eq!(holder.take_written(), expected);
}

/// REQ 3.DoIP-089 and 096: a registered tester asked whether it is alive may activate
/// its own address again first; that is answered at once, and its alive check response
/// behind it still counts.
#[test]
fn a_holder_that_reactivates_before_answering_is_alive() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let holder = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(holder.take_written(), alive_check_request());

    holder.send(&[activation_from(TESTER, 0), alive_check_response()].concat());
    assert_eq!(events(&mut entity), []);

    assert_eq!(
        holder.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(OTHER, 0x01)
    );
    assert!(!holder.is_shut());
}

/// REQ 4.DoIP-002: the reserve socket exchanging with an Initialized slot takes that
/// slot's unsent bytes with it: here, a NACK still queued for the idle tester.
#[test]
fn an_exchange_with_the_reserve_keeps_each_sockets_unsent_bytes() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let idle = stack.dial();
    let active = stack.dial();
    assert_eq!(events(&mut entity), []);
    idle.stall_writes();
    idle.send(&raw(0x4001, &[]));
    assert_eq!(events(&mut entity), []);

    active.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    assert_eq!(
        active.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
    idle.resume_writes();
    assert_eq!(events(&mut entity), []);
    assert_eq!(idle.take_written(), header_nack(0x01));
}

/// A connection with input always ready does not hold off the next connection's.
#[test]
fn a_busy_connection_does_not_starve_the_next() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let busy = activated(&stack, &mut entity, TESTER);
    let quiet = activated(&stack, &mut entity, OTHER);
    busy.send(&alive_check_response().repeat(400));
    quiet.send(&diagnostic(OTHER, ENTITY, &[0x3E, 0x00]));

    assert!(matches!(
        step(&mut entity),
        Some(Ev::Indication { connection: 1, .. })
    ));
    assert!(!busy.all_read());
}

/// The reserve socket is read while a connection has input always ready.
#[test]
fn a_busy_connection_does_not_starve_the_reserve() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let busy = activated(&stack, &mut entity, TESTER);
    let newcomer = stack.dial();
    assert_eq!(events(&mut entity), []);
    busy.send(&alive_check_response().repeat(400));
    newcomer.send(&activation_from(OTHER, 0));

    let mut buf = [0u8; 64];
    let next = pin!(entity.next_event(&mut buf, None));
    assert!(poll_times(next, 100).is_none());

    assert_eq!(busy.take_written(), alive_check_request());
    assert!(!busy.all_read());
}

/// The reserve socket is read while the first of several connections has input always
/// ready, however the turn wraps round the connections.
#[test]
fn a_busy_first_connection_does_not_starve_the_reserve() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let busy = activated(&stack, &mut entity, TESTER);
    let _idle = stack.dial();
    assert_eq!(events(&mut entity), []);
    let newcomer = stack.dial();
    assert_eq!(events(&mut entity), []);
    busy.send(&alive_check_response().repeat(400));
    newcomer.send(&activation_from(OTHER, 0));

    let mut buf = [0u8; 64];
    let next = pin!(entity.next_event(&mut buf, None));
    assert!(poll_times(next, 100).is_none());

    assert_eq!(
        newcomer.take_written(),
        activation_response_for(OTHER, ACTIVATED)
    );
    assert!(!busy.all_read());
}

/// An unregistered socket's unread NACKs do not keep it from moving into the reserve, so
/// a newcomer waiting there for a slot is registered at once: the entity's own frames
/// queue apart, in a queue every slot and the reserve have alike. The newcomer is then
/// alive-checked for the unregistered socket's own activation, the table being full
/// (REQ 3.DoIP-091).
#[test]
fn an_unregistered_socket_with_unread_nacks_does_not_hold_up_the_reserve() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let blocker = stack.dial();
    assert_eq!(events(&mut entity), []);
    blocker.stall_writes();
    for _ in 0..4 {
        blocker.send(&raw(0x4001, &[]));
    }
    assert_eq!(events(&mut entity), []);
    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    blocker.send(&activation_from(OTHER, 0));
    assert_eq!(events(&mut entity), []);

    let mut expected = activation_response_for(TESTER, ACTIVATED);
    expected.extend(alive_check_request());
    assert_eq!(newcomer.take_written(), expected);
    assert!(!blocker.is_shut());
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

/// REQ 7.DoIP-071: the entity answers its one functional address as well as its
/// physical one; another functional address is unknown, and refused with code 0x03.
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

/// A message longer than the caller's buffer is acknowledged and indicated truncated,
/// with its length: whether a message the caller cannot hold is an error is the
/// caller's to answer.
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
    assert_eq!(events(&mut entity), []);
    assert_eq!(peer.take_written(), ack(ENTITY, TESTER));
}

/// A connection is read while what the entity sends it waits for the tester to read:
/// neither direction holds the other up.
#[test]
fn a_connection_whose_writes_wait_is_still_read() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();
    request(&mut entity, TESTER, &[0x62; 8]).unwrap();
    peer.send(&diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));

    assert_eq!(
        events(&mut entity),
        [Ev::Indication {
            connection: 0,
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: vec![0x3E, 0x00],
        }]
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

/// ISO 13400-2:2019 8.3.1 and 8.3.2: a request is written to the connection that
/// registered its target, and confirmed once written.
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

/// ISO 13400-2:2019 8.3.2: a request whose target no connection registered is confirmed
/// `NoSocket`.
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

/// ISO 13400-2:2019 8.3.2: a request from a source address that is not the entity's
/// sends nothing, and is confirmed `UnknownSa`.
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

/// ISO 13400-2:2019 8.3.2: a request whose connection fails before it is written is
/// confirmed `NoSocket`, then the close reported.
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

/// ISO 13400-2:2019 8.3.1 and ISO 14229-2:2021 Table 10: a request with no room in its
/// connection's queue waits for nothing. It is confirmed `OutOfMemory` and never sent,
/// after the request ahead of it.
#[test]
fn a_request_with_no_room_is_confirmed_out_of_memory_after_the_one_ahead() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();

    request(&mut entity, TESTER, &[0x11; 30]).unwrap();
    request(&mut entity, TESTER, &[0x22; 30]).unwrap();
    assert_eq!(events(&mut entity), []);
    peer.resume_writes();

    assert_eq!(
        events(&mut entity),
        [
            confirm(ENTITY, TaType::Physical, DoIpResult::Ok),
            confirm(ENTITY, TaType::Physical, DoIpResult::OutOfMemory)
        ]
    );
    assert_eq!(peer.take_written(), diagnostic(ENTITY, TESTER, &[0x11; 30]));
}

/// The largest PDU a request takes is written whole.
#[test]
fn the_largest_pdu_is_requested_and_written() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();

    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    assert_eq!(
        peer.take_written(),
        diagnostic(ENTITY, TESTER, &[0x62; SMALL_PDU])
    );
}

/// The largest PDU a request takes is written whole when it answers a request at once,
/// with that request's acknowledgement still queued ahead of it (ISO 13400-2:2019 REQ
/// 7.DoIP-067 sends the acknowledgement first): a response is always preceded by one.
#[test]
fn the_largest_pdu_answering_a_request_is_written_after_its_acknowledgement() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();
    peer.send(&diagnostic(TESTER, ENTITY, &[0x22, 0xF1, 0x90]));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));

    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();
    peer.resume_writes();

    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    let mut written = ack(ENTITY, TESTER);
    written.extend(diagnostic(ENTITY, TESTER, &[0x62; SMALL_PDU]));
    assert_eq!(peer.take_written(), written);
}

/// The largest PDU is written whole behind any number of the entity's own frames: two
/// requests the tester pipelined have both acknowledgements queued when it is requested.
#[test]
fn the_largest_pdu_is_written_behind_two_acknowledgements() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();
    let mut pipelined = diagnostic(TESTER, ENTITY, &[0x22, 0xF1, 0x90]);
    pipelined.extend(diagnostic(TESTER, ENTITY, &[0x3E, 0x00]));
    peer.send(&pipelined);
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));
    assert!(matches!(step(&mut entity), Some(Ev::Indication { .. })));

    request(&mut entity, TESTER, &[0x62; SMALL_PDU]).unwrap();
    peer.resume_writes();

    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    let mut written = ack(ENTITY, TESTER);
    written.extend(ack(ENTITY, TESTER));
    written.extend(diagnostic(ENTITY, TESTER, &[0x62; SMALL_PDU]));
    assert_eq!(peer.take_written(), written);
}

/// REQ 3.DoIP-092: a tester that has stopped reading is replaced by one activating its
/// address once `T_TCP_Alive_Check` elapses, however full its queue. Its requests are
/// confirmed in order: the one it never read `NoSocket`, the one with no room
/// `OutOfMemory`.
#[test]
fn a_tester_that_stops_reading_is_replaced_after_t_tcp_alive_check() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());
    let stalled = activated(&stack, &mut entity, TESTER);
    stalled.stall_writes();
    request(&mut entity, TESTER, &[0x11; 30]).unwrap();
    request(&mut entity, TESTER, &[0x22; 30]).unwrap();

    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER, 0));
    assert_eq!(events(&mut entity), []);
    advance(ms(500));

    assert_eq!(
        events(&mut entity),
        [
            confirm(ENTITY, TaType::Physical, DoIpResult::NoSocket),
            confirm(ENTITY, TaType::Physical, DoIpResult::OutOfMemory)
        ]
    );
    assert!(stalled.is_aborted());
    assert_eq!(
        newcomer.take_written(),
        activation_response_for(TESTER, ACTIVATED)
    );
}

/// The `DiagnosticEntity` contract: requests to one target are confirmed in the order
/// they were made. One from another `sa`, settled at once, waits for the unwritten one
/// ahead of it.
#[test]
fn a_settled_request_is_confirmed_after_an_unwritten_one_ahead() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.stall_writes();
    request(&mut entity, TESTER, &[0x50, 0x01]).unwrap();
    {
        let future = pin!(entity.request(FUNCTIONAL, TESTER, TaType::Physical, &[0x50]));
        until_stalled(future).unwrap().unwrap();
    }
    assert_eq!(events(&mut entity), []);
    peer.resume_writes();

    assert_eq!(
        events(&mut entity),
        [
            confirm(ENTITY, TaType::Physical, DoIpResult::Ok),
            confirm(FUNCTIONAL, TaType::Physical, DoIpResult::UnknownSa)
        ]
    );
}

/// The `DiagnosticEntity` contract: a request made after an earlier one is confirmed is
/// still confirmed after every request made before it, wherever its confirm is held.
#[test]
fn a_later_request_is_confirmed_after_every_earlier_one() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    let first = diagnostic(ENTITY, TESTER, &[0x01]);
    peer.stall_writes_after(first.len() + 4);
    request(&mut entity, TESTER, &[0x01]).unwrap();
    request(&mut entity, TESTER, &[0x02; 40]).unwrap();
    assert_eq!(
        events(&mut entity),
        [confirm(ENTITY, TaType::Physical, DoIpResult::Ok)]
    );
    {
        let future = pin!(entity.request(ENTITY, TESTER, TaType::Functional, &[0x03]));
        until_stalled(future).unwrap().unwrap();
    }
    peer.resume_writes();

    assert_eq!(
        events(&mut entity),
        [
            confirm(ENTITY, TaType::Physical, DoIpResult::Ok),
            confirm(ENTITY, TaType::Functional, DoIpResult::Ok)
        ]
    );
}

/// A request is refused, with no confirm, where too many await theirs, or the PDU does
/// not fit or is empty (ISO 13400-2:2019 Table 21), whatever its source address.
#[test]
fn a_request_that_cannot_be_held_is_refused() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = Small::new(&stack, address(), two_testers());

    assert_eq!(
        request(&mut entity, TESTER, &[0; 60]),
        Err(Refusal::PduTooLarge {
            len: 60,
            max: SMALL_PDU
        })
    );
    assert_eq!(request(&mut entity, TESTER, &[]), Err(Refusal::EmptyPdu));
    {
        let not_ours = pin!(entity.request(OTHER, TESTER, TaType::Physical, &[]));
        assert_eq!(until_stalled(not_ours), Some(Err(Refusal::EmptyPdu)));
    }
    for _ in 0..4 {
        request(&mut entity, TESTER, &[0x7E, 0x00]).unwrap();
    }
    assert_eq!(
        request(&mut entity, TESTER, &[0x7E, 0x00]),
        Err(Refusal::NoRoom)
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

/// The `DiagnosticEntity` contract: closing a connection that has gone does nothing.
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

/// `Entity`'s cancel safety: a request dropped before it is polled was never made;
/// nothing is sent and nothing confirmed.
#[test]
fn a_request_dropped_unpolled_is_not_made() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    drop(entity.request(ENTITY, TESTER, TaType::Physical, &[0x62, 0x01]));

    assert_eq!(events(&mut entity), []);
    assert_eq!(peer.take_written(), []);
}

/// A close that has to wait out its own limits acts on no other socket's timer: another
/// tester's message that arrived in time is still indicated.
#[test]
fn a_waiting_close_leaves_other_sockets_timers_to_next_event() {
    let _clock = clock();
    let stack = MockStack::eager(4096);
    let mut entity = TwoSockets::new(&stack, address(), two_testers());
    let closing = activated(&stack, &mut entity, TESTER);
    let other = activated(&stack, &mut entity, OTHER);
    closing.send(&diagnostic(TESTER, ENTITY, &[0x11, 0x01]));
    other.send(&diagnostic(OTHER, ENTITY, &[0x3E, 0x00]));
    assert_eq!(events(&mut entity).len(), 2);

    advance(ms(299_800));
    other.send(&diagnostic(OTHER, ENTITY, &[0x3E, 0x00]));
    closing.stall_closes();
    closing.stall_aborts();
    {
        let mut close = pin!(entity.close(ConnectionId::new(0)));
        assert!(until_stalled(close.as_mut()).is_none());
        advance(ms(2000));
        assert!(until_stalled(close.as_mut()).is_none());
        advance(ms(500));
        until_stalled(close.as_mut()).unwrap().unwrap();
    }

    assert!(matches!(
        step(&mut entity),
        Some(Ev::Indication { connection: 1, .. })
    ));
}

/// ISO 14229-5:2022 REQ 7.11: a response confirmed before the caller's close is not
/// aborted away while the close can still deliver it, for the orderly close's 2 s.
#[test]
fn a_confirmed_response_is_given_the_orderly_close_to_arrive() {
    let _clock = clock();
    let stack = MockStack::eager(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x11, 0x01]));
    assert_eq!(events(&mut entity).len(), 1);
    peer.take_written();
    request(&mut entity, TESTER, &[0x51, 0x01]).unwrap();
    assert_eq!(
        step(&mut entity),
        Some(confirm(ENTITY, TaType::Physical, DoIpResult::Ok))
    );

    peer.stall_closes();
    let mut close = pin!(entity.close(ConnectionId::new(0)));
    assert!(until_stalled(close.as_mut()).is_none());
    advance(ms(1999));
    assert!(until_stalled(close.as_mut()).is_none());
    assert!(!peer.is_aborted());
    advance(ms(1));
    until_stalled(close.as_mut()).unwrap().unwrap();
    assert!(peer.is_aborted());
}

/// The `DiagnosticEntity` contract: closing an id no event has issued does nothing,
/// though a connection holds its slot.
#[test]
fn closing_an_id_no_event_issued_does_nothing() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);

    until_stalled(pin!(entity.close(ConnectionId::new(0))))
        .unwrap()
        .unwrap();

    assert_eq!(events(&mut entity), []);
    assert!(!peer.is_shut());
}

/// The `DiagnosticEntity` contract: no `Closed` follows a close, even of a connection
/// that had gone with its `Closed` still to be reported.
#[test]
fn no_closed_follows_closing_a_connection_that_had_gone() {
    let _clock = clock();
    let stack = MockStack::new(4096);
    let mut entity = OneSocket::new(&stack, address(), two_testers());
    let peer = activated(&stack, &mut entity, TESTER);
    peer.send(&diagnostic(TESTER, ENTITY, &[0x11, 0x01]));
    assert!(matches!(
        step(&mut entity),
        Some(Ev::Indication { connection: 0, .. })
    ));
    peer.take_written();
    peer.fail_writes();
    request(&mut entity, TESTER, &[0x51, 0x01]).unwrap();
    assert_eq!(
        step(&mut entity),
        Some(confirm(ENTITY, TaType::Physical, DoIpResult::NoSocket))
    );

    until_stalled(pin!(entity.close(ConnectionId::new(0))))
        .unwrap()
        .unwrap();

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
