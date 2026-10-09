//! `Entity`'s `next_event`, `request` and `close` lose nothing when dropped at any
//! await: each scenario runs once to completion, then again over sockets that move one
//! byte per read or write, dropping the future after every number of polls in turn, and
//! the events, the bytes on the wire and the datagrams sent must be the same.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used)]

mod support;

use std::net::SocketAddr;
use std::pin::pin;

use embassy_time::Duration;
use simple_doip::entity::{Entity, EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{ConnectionId, DiagnosticEntity, EntityConfig, EntityEvent};
use simple_doip::{EntityId, LogicalAddress, TaType, UDP_DISCOVERY_PORT, Vin};
use support::mock_stack::{
    ENTITY, MockStack, MockUdp, TESTER, advance, alive_check_response, clock, diagnostic,
    poll_times, raw, until_stalled,
};

const OTHER: LogicalAddress = LogicalAddress(0x0E80);

/// The most polls a future is given before it is dropped.
const MOST_POLLS: usize = 48;

type TestEntity<'a> = Entity<'a, MockStack, 1, 256, 2>;

fn new_entity(stack: &MockStack) -> TestEntity<'_> {
    TestEntity::new(
        stack,
        EntityAddress::new(ENTITY, LogicalAddress(0xE400)).unwrap(),
        EntityConfig::new(
            [TESTER, OTHER].map(|sa| simple_doip::service::TesterAddress::new(sa).unwrap()),
        ),
    )
}

fn activation_from(sa: LogicalAddress) -> Vec<u8> {
    let [high, low] = sa.0.to_be_bytes();
    raw(0x0005, &[high, low, 0, 0, 0, 0, 0])
}

/// How the scenario's futures are driven.
#[derive(Clone, Copy)]
enum Drive {
    /// Each to completion, or until it waits on the test.
    Whole,
    /// Each dropped after 1, 2, ... [`MOST_POLLS`] polls in turn, then driven whole once
    /// to see whether it is waiting on the test.
    Dropped,
}

/// An event in a form that outlives the caller's buffer.
fn describe(event: &EntityEvent<'_>) -> String {
    format!("{event:?}")
}

/// Every event `next_event` returns before it waits on the test.
fn drain<E: DiagnosticEntity>(entity: &mut E, drive: Drive, seen: &mut Vec<String>)
where
    E::Error: std::fmt::Debug,
{
    let mut polls = 1;
    loop {
        let mut buf = [0u8; 64];
        let future = pin!(entity.next_event(&mut buf, None));
        let event = match drive {
            Drive::Dropped if polls <= MOST_POLLS => {
                let budget = polls;
                polls = polls.checked_add(1).unwrap();
                match poll_times(future, budget) {
                    Some(event) => {
                        polls = 1;
                        event
                    }
                    None => continue,
                }
            }
            _ => {
                polls = 1;
                match until_stalled(future) {
                    Some(event) => event,
                    None => return,
                }
            }
        };
        seen.push(describe(&event.unwrap()));
    }
}

/// Closes `connection`, dropping the close after every number of polls in turn.
fn close(entity: &mut TestEntity<'_>, drive: Drive, connection: ConnectionId) {
    for polls in 1.. {
        let future = pin!(entity.close(connection));
        let done = match drive {
            Drive::Dropped if polls <= MOST_POLLS => poll_times(future, polls),
            _ => until_stalled(future),
        };
        if let Some(result) = done {
            return result.unwrap();
        }
    }
}

/// Requests `pdu` for `ta`. A request waits on nothing, so there is no future to drop.
fn request(entity: &mut TestEntity<'_>, ta: LogicalAddress, pdu: &[u8]) {
    entity.request(ENTITY, ta, TaType::Physical, pdu).unwrap();
}

/// Closes `connection` once its writes have stalled: the close waits out its limit,
/// then aborts.
fn close_stalled(entity: &mut TestEntity<'_>, drive: Drive, connection: ConnectionId) {
    for polls in 1.. {
        let future = pin!(entity.close(connection));
        let done = match drive {
            Drive::Dropped if polls <= MOST_POLLS => poll_times(future, polls),
            _ => match until_stalled(future) {
                None => {
                    advance(Duration::from_millis(500));
                    None
                }
                done => done,
            },
        };
        if let Some(result) = done {
            return result.unwrap();
        }
    }
}

/// A tester activates routing with a diagnostic message right behind it, is answered,
/// and the entity closes the connection after its answer.
fn activate_echo_close(drive: Drive) -> (Vec<String>, Vec<u8>, bool) {
    let stack = MockStack::new(1);
    let mut entity = new_entity(&stack);
    let peer = stack.dial();
    peer.send(
        &[
            activation_from(TESTER),
            diagnostic(TESTER, ENTITY, &[0x11, 0x01]),
        ]
        .concat(),
    );
    let mut seen = Vec::new();

    drain(&mut entity, drive, &mut seen);
    request(&mut entity, TESTER, &[0x51, 0x01]);
    drain(&mut entity, drive, &mut seen);
    close(&mut entity, drive, ConnectionId::new(0));
    drain(&mut entity, drive, &mut seen);

    (seen, peer.take_written(), peer.is_closed())
}

/// A second tester claims a registered source address on the reserve socket, the
/// holder answers its alive check, and the second is refused.
fn contested_activation(drive: Drive) -> (Vec<String>, Vec<u8>, Vec<u8>) {
    let stack = MockStack::new(1);
    let mut entity = new_entity(&stack);
    let holder = stack.dial();
    holder.send(&activation_from(TESTER));
    let mut seen = Vec::new();
    drain(&mut entity, drive, &mut seen);

    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER));
    drain(&mut entity, drive, &mut seen);
    holder.send(&alive_check_response());
    drain(&mut entity, drive, &mut seen);

    (seen, holder.take_written(), newcomer.take_written())
}

/// A second tester claims a registered source address, the holder stays silent, and
/// after `T_TCP_Alive_Check` the holder is aborted and the second registered.
fn silent_holder_replaced(drive: Drive) -> (Vec<String>, Vec<u8>, Vec<u8>, bool) {
    let stack = MockStack::new(1);
    let mut entity = new_entity(&stack);
    let holder = stack.dial();
    holder.send(&activation_from(TESTER));
    let mut seen = Vec::new();
    drain(&mut entity, drive, &mut seen);

    let newcomer = stack.dial();
    newcomer.send(&activation_from(TESTER));
    drain(&mut entity, drive, &mut seen);
    advance(Duration::from_millis(500));
    drain(&mut entity, drive, &mut seen);

    (
        seen,
        holder.take_written(),
        newcomer.take_written(),
        holder.is_aborted(),
    )
}

/// A registered tester stops reading with a response queued, and the entity's close
/// of the connection is aborted once it cannot finish.
fn stalled_close_aborted(drive: Drive) -> (Vec<String>, Vec<u8>, bool) {
    let stack = MockStack::new(1);
    let mut entity = new_entity(&stack);
    let peer = stack.dial();
    peer.send(
        &[
            activation_from(TESTER),
            diagnostic(TESTER, ENTITY, &[0x11, 0x01]),
        ]
        .concat(),
    );
    let mut seen = Vec::new();
    drain(&mut entity, drive, &mut seen);
    peer.stall_writes();

    request(&mut entity, TESTER, &[0x51, 0x01]);
    close_stalled(&mut entity, drive, ConnectionId::new(0));
    drain(&mut entity, drive, &mut seen);

    (seen, peer.take_written(), peer.is_aborted())
}

/// A datagram the entity sent: the step it was sent at, where to, and the frame.
type Datagram = (u32, SocketAddr, Vec<u8>);

/// The entity announces itself while testers ask it to identify itself, plainly and by
/// EID and VIN, each once the last is answered, and ask its power mode and status:
/// every datagram it sends, and when.
fn discovery_burst(drive: Drive) -> (Vec<String>, Vec<Datagram>) {
    const EID: [u8; 6] = [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF];
    const VIN: [u8; 17] = *b"WVWZZZ1JZXW000001";
    let stack = MockStack::new(1);
    let udp = MockUdp::new();
    let identity =
        FixedIdentity::new(EntityId::new(EID).unwrap(), DiagnosticPowerModeCode::Ready)
            .with_vin(Vin::new(VIN).unwrap());
    let mut entity = new_entity(&stack).with_discovery(udp.socket(), identity, 0x1234_5678);
    let tester: SocketAddr = ([192, 168, 0, 2], UDP_DISCOVERY_PORT).into();
    let mut seen = Vec::new();
    let mut sent = Vec::new();

    for step in 0..=20 {
        match step {
            1 => udp.deliver(tester, &raw(0x0001, &[])),
            4 => udp.deliver(tester, &raw(0x4003, &[])),
            7 => udp.deliver(tester, &raw(0x0002, &EID)),
            8 => udp.deliver(tester, &raw(0x4001, &[])),
            13 => udp.deliver(tester, &raw(0x0003, &VIN)),
            _ => {}
        }
        drain(&mut entity, drive, &mut seen);
        sent.extend(
            udp.take_sent()
                .into_iter()
                .map(|(to, frame)| (step, to, frame)),
        );
        advance(Duration::from_millis(100));
    }

    (seen, sent)
}

#[test]
fn next_event_and_close_lose_nothing_when_dropped_at_any_await() {
    let _clock = clock();
    let whole = activate_echo_close(Drive::Whole);
    assert_eq!(
        whole.0.len(),
        2,
        "an indication and a confirm: {:?}",
        whole.0
    );
    assert!(whole.2);

    assert_eq!(activate_echo_close(Drive::Dropped), whole);
}

#[test]
fn the_socket_handler_loses_nothing_when_dropped_at_any_await() {
    let _clock = clock();
    let whole = contested_activation(Drive::Whole);
    assert!(!whole.2.is_empty(), "the newcomer is answered");

    assert_eq!(contested_activation(Drive::Dropped), whole);
}

/// A connection the acceptor has made is never lost to a dropped `next_event`.
#[test]
fn no_connection_is_lost_to_a_dropped_accept() {
    let _clock = clock();
    let stack = MockStack::new(1);
    let mut entity = new_entity(&stack);
    let peers = [stack.dial(), stack.dial()];
    let mut seen = Vec::new();

    drain(&mut entity, Drive::Dropped, &mut seen);
    let [first, second] = &peers;
    first.send(&activation_from(TESTER));
    second.send(&activation_from(LogicalAddress(0x0E01)));
    drain(&mut entity, Drive::Dropped, &mut seen);

    let answered = peers
        .iter()
        .filter(|peer| !peer.take_written().is_empty())
        .count();
    assert_eq!(answered, 2, "both connections were accepted and answered");
}

#[test]
fn an_alive_check_that_expires_loses_nothing_when_dropped_at_any_await() {
    let first_run = clock();
    let whole = silent_holder_replaced(Drive::Whole);
    assert!(whole.3, "the silent holder is aborted");
    assert!(!whole.2.is_empty(), "the newcomer is answered");

    drop(first_run);
    let _clock = clock();
    assert_eq!(silent_holder_replaced(Drive::Dropped), whole);
}

#[test]
fn a_close_that_cannot_finish_loses_nothing_when_dropped_at_any_await() {
    let first_run = clock();
    let whole = stalled_close_aborted(Drive::Whole);
    assert!(whole.2, "the stalled close is aborted");

    drop(first_run);
    let _clock = clock();
    assert_eq!(stalled_close_aborted(Drive::Dropped), whole);
}

#[test]
fn discovery_loses_nothing_when_dropped_at_any_await() {
    let first_run = clock();
    let whole = discovery_burst(Drive::Whole);
    assert!(whole.0.is_empty(), "UDP raises no event: {:?}", whole.0);
    assert_eq!(
        whole.1.len(),
        8,
        "three announcements and five answers: {:?}",
        whole.1
    );

    drop(first_run);
    let _clock = clock();
    assert_eq!(discovery_burst(Drive::Dropped), whole);
}
