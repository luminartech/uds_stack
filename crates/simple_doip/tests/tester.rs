//! `Tester` over a scripted `edge-nal` backend: routing activation, requests and their
//! confirms, indications, alive checks, reconnection, and cancellation at every await.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::net::SocketAddr;
use std::pin::pin;

use embassy_time::Duration;
use simple_doip::messages::{NackCode, RoutingActivationResponseCode};
use simple_doip::service::{ConnectionEvent, DiagnosticConnection, NotATesterAddress};
use simple_doip::tester::{Error, Tester};
use simple_doip::{LogicalAddress, TaType};
use support::mock_stack::*;

const N: usize = 64;
const REMOTE: SocketAddr = SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    13400,
));

fn connect(stack: &MockStack) -> Result<Tester<'_, MockStack, N>, Error<MockError>> {
    run(Tester::connect(stack, REMOTE, TESTER))
}

/// ISO 13400-2:2019 Table 46 and Table 47: a default (`0x00`) activation from the
/// tester's address; REQ 3.DoIP-062: `0x10` activates routing.
#[test]
fn activation_sends_the_request_and_0x10_activates() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x10));

    let tester = connect(&stack);

    assert!(tester.is_ok(), "{tester:?}");
    assert_eq!(stack.latest().take_written(), activation_request());
    assert!(!stack.latest().is_shut());
}

/// ISO 13400-2:2019 Table 49, REQ 3.DoIP-059, 060, 149, 150, 061, 105, 151 and 174: every
/// code other than `0x10` and `0x11` refuses routing, and the tester gives up the socket.
#[test]
fn every_denial_code_is_an_error_and_the_socket_is_given_up() {
    let _clock = clock();
    for code in [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0F, 0x12, 0xDF, 0xE0, 0xFE,
        0xFF,
    ] {
        let stack = MockStack::new(usize::MAX);
        stack.script_next(&activation_response(code));

        let error = connect(&stack).unwrap_err();

        assert_eq!(
            error,
            Error::RoutingActivationDenied(RoutingActivationResponseCode::from(code)),
            "{code:#04X}"
        );
        assert!(stack.latest().is_shut(), "{code:#04X}");
    }
}

/// ISO 13400-2:2019 Table 49: after `0x04` the entity keeps the socket open, so the
/// tester, which implements no authentication, closes it itself.
#[test]
fn missing_authentication_is_denied_and_closed_by_the_tester() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x04));

    let error = connect(&stack).unwrap_err();

    assert_eq!(
        error,
        Error::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedMissingAuthentication
        )
    );
    assert!(stack.latest().is_shut());
}

/// ISO 13400-2:2019 REQ 3.DoIP-063, NOTE 3: after `0x11` the tester repeats the request
/// on the same socket until confirmation completes.
#[test]
fn confirmation_required_is_retried_on_the_same_socket_until_activated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x11));
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, TESTER));

    assert!(until_stalled(connecting.as_mut()).is_none());
    assert_eq!(stack.latest().take_written(), activation_request());

    advance(Duration::from_millis(1999));
    assert!(until_stalled(connecting.as_mut()).is_none());
    assert_eq!(stack.latest().take_written(), []);

    advance(Duration::from_millis(1));
    assert!(until_stalled(connecting.as_mut()).is_none());
    assert_eq!(stack.latest().take_written(), activation_request());

    stack.latest().send(&activation_response(0x10));
    assert!(run(connecting).is_ok());
    assert_eq!(stack.connects(), 1);
}

/// ISO 13400-2:2019 REQ 3.DoIP-105: a confirmation the vehicle rejects ends the retries.
#[test]
fn confirmation_required_then_rejected_is_denied() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x11));
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, TESTER));
    assert!(until_stalled(connecting.as_mut()).is_none());

    advance(Duration::from_secs(2));
    assert!(until_stalled(connecting.as_mut()).is_none());
    stack.latest().send(&activation_response(0x05));

    assert_eq!(
        run(connecting).unwrap_err(),
        Error::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedRejectedConfirmation
        )
    );
}

/// ISO 13400-2:2019 Table 28 and REQ 3.DoIP-134: a socket pending confirmation is
/// registered, so the entity may check it is alive, and the tester answers.
#[test]
fn an_alive_check_during_activation_is_answered() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut script = alive_check_request();
    script.extend(activation_response(0x10));
    stack.script_next(&script);

    assert!(connect(&stack).is_ok());

    let mut expected = activation_request();
    expected.extend(alive_check_response());
    assert_eq!(stack.latest().take_written(), expected);
}

/// ISO 13400-2:2019 Table 48: the response names the tester that asked; one naming
/// another is not this tester's activation.
#[test]
fn a_response_for_another_tester_is_an_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response_for(LogicalAddress(0x0E01), 0x10));

    assert_eq!(
        connect(&stack).unwrap_err(),
        Error::ActivationAnsweredForAnotherTester(LogicalAddress(0x0E01))
    );
    assert!(stack.latest().is_shut());
}

/// ISO 13400-2:2019 Table 19: the entity rejected the request's header.
#[test]
fn a_header_nack_during_activation_is_an_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&header_nack(0x02));

    assert_eq!(
        connect(&stack).unwrap_err(),
        Error::HeaderNack(NackCode::MessageTooLarge)
    );
    assert!(stack.latest().is_shut());
}

#[test]
fn the_entity_closing_during_activation_is_an_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, TESTER));
    assert!(until_stalled(connecting.as_mut()).is_none());

    stack.latest().eof();

    assert_eq!(run(connecting).unwrap_err(), Error::ClosedDuringActivation);
}

#[test]
fn a_refused_connection_is_an_io_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.refuse_next_connect();

    assert_eq!(connect(&stack).unwrap_err(), Error::Io(MockError));
}

/// ISO 13400-2:2019 Table 13: a tester's source address is in the client range, so
/// anything else is refused before a connection is attempted.
#[test]
fn a_source_address_outside_the_client_range_is_refused_before_connecting() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);

    let error = run(Tester::<_, N>::connect(
        &stack,
        REMOTE,
        LogicalAddress(0xE400),
    ));

    assert_eq!(
        error.unwrap_err(),
        Error::NotATesterAddress(NotATesterAddress {
            address: LogicalAddress(0xE400)
        })
    );
    assert_eq!(stack.connects(), 0);
}

// --- next_event ------------------------------------------------------------------------

type ActiveTester<'s> = Tester<'s, MockStack, N>;

/// A tester with routing active over `stack`, its activation request already taken.
fn active(stack: &MockStack) -> ActiveTester<'_> {
    stack.script_next(&activation_response(0x10));
    let tester = connect(stack).unwrap();
    stack.latest().take_written();
    tester
}

fn next<'b>(
    tester: &mut ActiveTester<'_>,
    buf: &'b mut [u8],
) -> Result<ConnectionEvent<'b>, Error<MockError>> {
    run(tester.next_event(buf, None))
}

/// ISO 13400-2:2019 8.3.3: a diagnostic message is `DoIP_Data.indication`, its target's
/// addressing model derived from the address (Table 13).
#[test]
fn a_diagnostic_message_is_indicated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    stack
        .latest()
        .send(&diagnostic(ENTITY, LogicalAddress(0xE400), &[0x7E, 0x80]));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[0x7E, 0x00],
        }
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: LogicalAddress(0xE400),
            ta_type: TaType::Functional,
            pdu: &[0x7E, 0x80],
        }
    );
}

#[test]
fn a_message_longer_than_the_callers_buffer_is_indicated_truncated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[1, 2, 3, 4, 5]));
    let mut buf = [0; 3];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::IndicationTruncated {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[1, 2, 3],
            length: 5,
        }
    );
}

/// A message longer than the tester's own buffer is indicated as far as it was buffered,
/// and the message after it arrives whole.
#[test]
fn a_message_longer_than_the_testers_buffer_is_truncated_and_the_stream_stays_in_sync() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let long: Vec<u8> = (0..100).collect();
    stack.latest().send(&diagnostic(ENTITY, TESTER, &long));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 128];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::IndicationTruncated {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &long[..N - 12],
            length: 100,
        }
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[0x7E, 0x00],
        }
    );
}

/// ISO 13400-2:2019 8.3.3: a diagnostic message in error raises no indication.
#[test]
fn a_diagnostic_message_too_short_for_its_addresses_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&raw(0x8001, &[0x00, 0x01, 0x0E]));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 16];

    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            pdu: [0x7E, 0x00],
            ..
        }
    ));
}

/// ISO 14229-5:2022 REQ 7.7 defines payload type `0x8004`, which ISO 13400-2:2019 Table 17
/// reserves: a valid message this crate does not model is reported, not dropped.
#[test]
fn payload_type_0x8004_is_unmodelled() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&raw(0x8004, &[0x00, 0x01, 0x0E, 0x00, 0x6A, 0x01]));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Unmodelled {
            payload_type: 0x8004,
            data: &[0x00, 0x01, 0x0E, 0x00, 0x6A, 0x01],
        }
    );
}

/// ISO 13400-2:2019 Table 28: the tester answers an alive check with its own address,
/// and reports nothing for it.
#[test]
fn an_alive_check_request_is_answered_with_the_testers_address() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&alive_check_request());
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 16];

    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));
    assert!(until_stalled(waiting.as_mut()).is_none());
    assert_eq!(stack.latest().take_written(), alive_check_response());
}

#[test]
fn the_callers_deadline_is_reported_when_nothing_arrives_first() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(250)));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(249));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

#[test]
fn a_deadline_already_passed_is_reported_at_once() {
    let _clock = clock();
    advance(Duration::from_secs(5));
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let mut buf = [0; 16];

    assert_eq!(
        run(tester.next_event(&mut buf, Some(1_000))).unwrap(),
        ConnectionEvent::Deadline
    );
}

/// `deadline_ms` is the clock's milliseconds truncated to 32 bits: a deadline just past
/// the wrap is ahead, not behind.
#[test]
fn a_deadline_across_the_u32_wrap_is_ahead() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    advance(Duration::from_millis(u64::from(u32::MAX) - 9));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(20)));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(29));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

/// `now` reads the clock the tester's own timers run on, so a deadline computed from it
/// is the instant the tester waits for, across the 32-bit wrap as anywhere.
#[test]
fn a_deadline_from_now_is_on_the_testers_clock() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    advance(Duration::from_millis(u64::from(u32::MAX) + 1 + 5_000));
    assert_eq!(tester.now(), 5_000);
    let deadline = tester.now().wrapping_add(100);
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(deadline)));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(99));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

#[test]
fn the_entity_closing_is_closed_once_then_not_connected() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap_err(),
        Error::NotConnected
    );
}

/// ISO 13400-2:2019 Table 19: a header out of sync cannot be skipped. The tester, which
/// must not answer it with a negative acknowledgement (REQ 7.DoIP-040), closes.
#[test]
fn a_header_out_of_sync_closes_the_connection() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&[0x03, 0xFD, 0x80, 0x01, 0, 0, 0, 4, 0, 1, 0x0E, 0]);
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert!(stack.latest().is_shut());
    assert_eq!(stack.latest().take_written(), []);
}

#[test]
fn a_failed_read_is_an_error_then_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().fail_reads();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap_err(),
        Error::Io(MockError)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap_err(),
        Error::NotConnected
    );
}
