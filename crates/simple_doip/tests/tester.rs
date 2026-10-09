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
use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, NotATesterAddress, Reconnection,
    Refusal, TesterAddress, TesterConnection, Timestamp,
};
use simple_doip::tester::{ConnectError, MIN_N, RECONNECT_BACKOFF, Tester};
use simple_doip::{LogicalAddress, TaType};
use support::mock_stack::*;

const N: usize = 64;

/// `A_DoIP_Diagnostic_Message` (ISO 13400-2:2019 Table 12).
const ACK_TIMEOUT: Duration = Duration::from_secs(2);
const REMOTE: SocketAddr = SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    13400,
));

fn sa() -> TesterAddress {
    TesterAddress::new(TESTER).unwrap()
}

fn connect(stack: &MockStack) -> Result<Tester<'_, MockStack, N>, ConnectError<MockError>> {
    run(Tester::connect(stack, REMOTE, sa()))
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
            ConnectError::RoutingActivationDenied(RoutingActivationResponseCode::from(
                code
            )),
            "{code:#04X}"
        );
        assert!(stack.latest().is_aborted(), "{code:#04X}");
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
        ConnectError::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedMissingAuthentication
        )
    );
    assert!(stack.latest().is_aborted());
}

/// ISO 13400-2:2019 REQ 3.DoIP-063, NOTE 3: after `0x11` the tester repeats the request
/// on the same socket until confirmation completes.
#[test]
fn confirmation_required_is_retried_on_the_same_socket_until_activated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x11));
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, sa()));

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
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, sa()));
    assert!(until_stalled(connecting.as_mut()).is_none());

    advance(Duration::from_secs(2));
    assert!(until_stalled(connecting.as_mut()).is_none());
    stack.latest().send(&activation_response(0x05));

    assert_eq!(
        run(connecting).unwrap_err(),
        ConnectError::RoutingActivationDenied(
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

/// `Tester::connect`'s contract: only a routing activation response answers the request,
/// and any other well-formed message while it waits is passed over.
#[test]
fn other_messages_during_activation_are_passed_over() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut script = alive_check_response();
    script.extend(nack(ENTITY, TESTER, 0x02));
    script.extend(activation_response(0x10));
    stack.script_next(&script);

    assert!(connect(&stack).is_ok());
    assert_eq!(stack.latest().take_written(), activation_request());
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
        ConnectError::ActivationAnsweredForAnotherTester(LogicalAddress(0x0E01))
    );
    assert!(stack.latest().is_aborted());
}

/// ISO 13400-2:2019 Table 19: the entity rejected the request's header.
#[test]
fn a_header_nack_during_activation_is_an_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&header_nack(0x02));

    assert_eq!(
        connect(&stack).unwrap_err(),
        ConnectError::HeaderNack(NackCode::MessageTooLarge)
    );
    assert!(stack.latest().is_aborted());
}

#[test]
fn the_entity_closing_during_activation_is_an_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut connecting = pin!(Tester::<_, N>::connect(&stack, REMOTE, sa()));
    assert!(until_stalled(connecting.as_mut()).is_none());

    stack.latest().eof();

    assert_eq!(run(connecting).unwrap_err(), ConnectError::Closed);
}

#[test]
fn a_refused_connection_is_an_io_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.refuse_next_connect();

    assert_eq!(connect(&stack).unwrap_err(), ConnectError::Io(MockError));
}

/// ISO 13400-2:2019 Table 13: a tester's source address is in the client range, so
/// nothing else can be made into one.
#[test]
fn a_source_address_outside_the_client_range_is_not_a_tester_address() {
    const SA: TesterAddress = match TesterAddress::new(LogicalAddress(0x0E00)) {
        Ok(sa) => sa,
        Err(_) => panic!("0x0E00 is a tester address"),
    };
    for address in [0x0DFF, 0x1000, 0xE400] {
        assert_eq!(
            TesterAddress::new(LogicalAddress(address)),
            Err(NotATesterAddress {
                address: LogicalAddress(address)
            })
        );
    }
    for address in [0x0E00, 0x0FFF] {
        assert_eq!(
            TesterAddress::new(LogicalAddress(address)).map(TesterAddress::address),
            Ok(LogicalAddress(address))
        );
    }
    assert_eq!(TesterAddress::try_from(LogicalAddress(0x0E00)), Ok(SA));
    assert_eq!(SA.to_string(), LogicalAddress(0x0E00).to_string());
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

/// Reconnects, letting [`RECONNECT_BACKOFF`] pass on the mock clock.
fn reconnect(tester: &mut ActiveTester<'_>) -> Result<(), ConnectError<MockError>> {
    let mut reconnecting = pin!(tester.reconnect(None));
    let reconnected = if let Some(done) = until_stalled(reconnecting.as_mut()) {
        done
    } else {
        advance(RECONNECT_BACKOFF);
        run(reconnecting)
    };
    reconnected.map(|connected| assert_eq!(connected, Reconnection::Connected))
}

fn next<'b>(
    tester: &mut ActiveTester<'_>,
    buf: &'b mut [u8],
) -> Result<ConnectionEvent<'b>, core::convert::Infallible> {
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
    let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(250))));

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
        run(tester.next_event(&mut buf, Some(Timestamp(1_000)))).unwrap(),
        ConnectionEvent::Deadline
    );
}

/// A deadline is the clock's milliseconds truncated to 32 bits: a deadline just past
/// the wrap is ahead, not behind.
#[test]
fn a_deadline_across_the_u32_wrap_is_ahead() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    advance(Duration::from_millis(u64::from(u32::MAX) - 9));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(20))));

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
    assert_eq!(tester.now(), Timestamp(5_000));
    let deadline = tester.now().after(100);
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

/// A connection ending is an event, never an `Err`: `Closed` once, then nothing but
/// the caller's deadline until a reconnect succeeds.
#[test]
fn the_entity_closing_is_closed_once_until_a_reconnect() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_quiet_until_the_deadline(&mut tester);
    assert_eq!(tester.io_error(), None);
    assert!(stack.latest().is_shut());
    assert!(
        !stack.latest().is_aborted(),
        "the entity ended it; nothing to abort"
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
    assert!(stack.latest().is_aborted());
    assert_eq!(stack.latest().take_written(), []);
}

/// Why the connection was lost, as a layer generic over [`TesterConnection`] sees it.
fn cause_of_loss<T: TesterConnection>(connection: &T) -> Option<&T::IoError> {
    connection.io_error()
}

/// A layer that only knows the connection as a [`TesterConnection`] can still tell a
/// failed socket from an entity that closed it.
#[test]
fn the_cause_of_a_loss_is_readable_through_the_trait() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let mut buf = [0; 16];
    stack.latest().eof();
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_eq!(cause_of_loss(&tester), None, "the entity closed it");

    stack.script_next(&activation_response(0x10));
    reconnect(&mut tester).unwrap();
    stack.latest().fail_reads();
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_eq!(cause_of_loss(&tester), Some(&MockError));
}

/// A failed read ends the connection, reported as `Closed` rather than as an `Err`; the
/// socket's error stays readable from the tester.
#[test]
fn a_failed_read_is_closed_and_keeps_its_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().fail_reads();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_quiet_until_the_deadline(&mut tester);
    assert_eq!(tester.io_error(), Some(&MockError));
    assert!(stack.latest().is_shut());
    assert!(
        !stack.latest().is_aborted(),
        "the socket failed; nothing to abort"
    );
}

// --- request and its confirm -----------------------------------------------------------

const PDU: [u8; 3] = [0x22, 0xF1, 0x90];

fn request(tester: &mut ActiveTester<'_>) -> Result<(), Refusal> {
    tester.request(ENTITY, TaType::Physical, &PDU)
}

/// A closed tester whose `Closed` was reported waits for the caller's deadline, and
/// with none, for good: nothing can arrive before a reconnect.
fn assert_quiet_until_the_deadline(tester: &mut ActiveTester<'_>) {
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
    }
    let deadline = tester.now().after(100);
    let mut waiting = pin!(tester.next_event(&mut buf, Some(deadline)));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(100));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

fn confirm(result: DoIpResult) -> ConnectionEvent<'static> {
    ConnectionEvent::Confirm {
        sa: TESTER,
        ta: ENTITY,
        ta_type: TaType::Physical,
        result,
    }
}

/// ISO 13400-2:2019 Table 21: the request is one diagnostic message from the tester's
/// source address to the requested target, written by `next_event`.
#[test]
fn a_request_is_written_as_one_diagnostic_message() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);

    request(&mut tester).unwrap();
    assert_eq!(stack.latest().take_written(), []);
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));
    assert!(until_stalled(waiting.as_mut()).is_none());

    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &PDU)
    );
}

/// ISO 13400-2:2019 Table 23, and ISO 14229-5:2022 clause 11: the positive
/// acknowledgement is the request's `DoIP_Data.confirm`, with the request's addressing.
#[test]
fn a_positive_ack_confirms_ok() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// `Tester`'s contract: a diagnostic message is indicated when it arrives, so a response
/// the entity sends before its acknowledgement is not lost, and the acknowledgement still
/// confirms the request.
#[test]
fn a_response_before_its_ack_is_indicated_and_the_ack_confirms() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x62, 0xF1, 0x90]));
    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[0x62, 0xF1, 0x90],
        }
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// ISO 13400-2:2019 Table 26 against 8.2.5: each negative acknowledgement code confirms
/// with the `DoIP_Result` that names it, and `DoIP_ERROR` where none does.
#[test]
fn every_nack_code_confirms_its_result() {
    let _clock = clock();
    for (code, result) in [
        (0x02, DoIpResult::InvalidSa),
        (0x03, DoIpResult::UnknownTa),
        (0x04, DoIpResult::MessageTooLarge),
        (0x05, DoIpResult::OutOfMemory),
        (0x06, DoIpResult::TargetUnreachable),
        (0x07, DoIpResult::Error),
        (0x08, DoIpResult::Error),
        (0x99, DoIpResult::Error),
    ] {
        let stack = MockStack::new(usize::MAX);
        let mut tester = active(&stack);
        request(&mut tester).unwrap();
        stack.latest().send(&nack(ENTITY, TESTER, code));
        let mut buf = [0; 16];

        assert_eq!(
            next(&mut tester, &mut buf).unwrap(),
            confirm(result),
            "{code:#04X}"
        );
    }
}

/// ISO 13400-2:2019 Table 19 and REQ 7.DoIP-040: a generic header negative
/// acknowledgement tells the tester what was wrong with the message it just sent.
#[test]
fn a_header_nack_confirms_the_outstanding_request() {
    let _clock = clock();
    for (code, result) in [
        (0x00, DoIpResult::HdrError),
        (0x01, DoIpResult::HdrError),
        (0x02, DoIpResult::MessageTooLarge),
        (0x03, DoIpResult::OutOfMemory),
        (0x04, DoIpResult::HdrError),
        (0x05, DoIpResult::Error),
    ] {
        let stack = MockStack::new(usize::MAX);
        let mut tester = active(&stack);
        request(&mut tester).unwrap();
        stack.latest().send(&header_nack(code));
        let mut buf = [0; 16];

        assert_eq!(
            next(&mut tester, &mut buf).unwrap(),
            confirm(result),
            "{code:#04X}"
        );
    }
}

/// ISO 13400-2:2019 Table 12: `A_DoIP_Diagnostic_Message` — after 2 s without an
/// acknowledgement the request is considered lost, which is `DoIP_TIMEOUT_A`.
#[test]
fn no_ack_within_a_doip_diagnostic_message_is_timeout_a() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1999));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::TimeoutA)))
    );
}

/// ISO 13400-2:2019 Table 12: `A_DoIP_Diagnostic_Message` is the whole 2 s, so an
/// acknowledgement late in it still confirms the request.
#[test]
fn an_ack_late_in_a_doip_diagnostic_message_confirms_ok() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1999));
    stack.latest().send(&ack(ENTITY, TESTER));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::Ok)))
    );
}

/// `A_DoIP_Diagnostic_Message` runs from the request's last byte: a request whose writing
/// took a while gets its whole window after it.
#[test]
fn the_ack_timer_starts_at_the_last_byte_written() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes_after(5);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1500));
    stack.latest().resume_writes();
    assert!(until_stalled(waiting.as_mut()).is_none());
    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &PDU)
    );
    advance(Duration::from_millis(1999));
    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(1));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::TimeoutA)))
    );
}

#[test]
fn an_ack_for_another_tester_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().send(&ack(ENTITY, LogicalAddress(0x0E01)));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_secs(2));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::TimeoutA)))
    );
}

/// ISO 13400-2:2019 Table 23 gives the acknowledgement's source as the request's
/// intended receiver, which for a functional request is no single address; the
/// tester does not hold it to the requested target.
#[test]
fn a_functional_requests_ack_from_any_source_confirms() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    tester
        .request(LogicalAddress(0xE400), TaType::Functional, &PDU)
        .unwrap();
    stack.latest().send(&ack(LogicalAddress(0x0002), TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Confirm {
            sa: TESTER,
            ta: LogicalAddress(0xE400),
            ta_type: TaType::Functional,
            result: DoIpResult::Ok,
        }
    );
}

#[test]
fn an_acknowledgement_with_nothing_outstanding_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&ack(ENTITY, TESTER));
    stack.latest().send(&header_nack(0x02));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 16];

    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));
}

/// One request is outstanding at a time; another before its confirm is not accepted,
/// and gets no confirm of its own.
#[test]
fn a_second_request_before_the_confirm_is_refused() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();

    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NoRoom);

    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &PDU)
    );
    request(&mut tester).unwrap();
}

/// `N` bounds the whole message: a PDU of [`DiagnosticConnection::MAX_PDU`] is sent, a
/// longer one is not accepted.
#[test]
fn a_pdu_longer_than_max_pdu_is_refused() {
    const MAX_PDU: usize = <Tester<'_, MockStack, N> as DiagnosticConnection>::MAX_PDU;
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    assert_eq!(MAX_PDU, N - 12);

    assert_eq!(
        tester
            .request(ENTITY, TaType::Physical, &[0x2E; MAX_PDU + 1])
            .unwrap_err(),
        Refusal::PduTooLarge {
            len: MAX_PDU + 1,
            max: MAX_PDU
        }
    );
    tester
        .request(ENTITY, TaType::Physical, &[0x2E; MAX_PDU])
        .unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));
    assert!(until_stalled(waiting.as_mut()).is_none());
    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &[0x2E; MAX_PDU])
    );
}

/// ISO 13400-2:2019 Table 21: a diagnostic message carries at least one byte of user
/// data, so an empty PDU is refused rather than sent for the entity to NACK and close
/// the connection over.
#[test]
fn an_empty_pdu_is_refused() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);

    assert_eq!(
        tester.request(ENTITY, TaType::Physical, &[]).unwrap_err(),
        Refusal::EmptyPdu
    );
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
    }
    assert_eq!(stack.latest().take_written(), []);
    request(&mut tester).unwrap();
}

#[test]
fn a_request_when_closed_is_not_connected() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );

    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NotConnected);
}

/// The address routing activation registered, which a caller checks an indication's
/// target against.
#[test]
fn a_tester_reports_the_address_it_activated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let tester = active(&stack);

    assert_eq!(tester.address().address(), TESTER);
}

/// A request that was written but never acknowledged is confirmed as failed before the
/// close is reported, so the layer above is never left waiting.
#[test]
fn the_entity_closing_with_a_request_outstanding_confirms_before_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().eof();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Error)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// ISO 14229-5:2022 REQ 7.9: a server that changes session closes the connection right
/// after its positive response. What arrived before the close is reported first: the
/// request's confirm, the response, then `Closed`.
#[test]
fn what_arrived_before_the_entity_closed_is_reported_before_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    assert!(until_stalled(pin!(tester.next_event(&mut buf, None))).is_none());
    stack.latest().send(&ack(ENTITY, TESTER));
    stack.latest().send(&diagnostic(
        ENTITY,
        TESTER,
        &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4],
    ));
    stack.latest().eof();

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4],
        }
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// ISO 13400-2:2019 8.2.5: a request whose last byte never left had no socket to carry
/// it, so a failed write confirms it `DoIP_NO_SOCKET`, then reports the close.
#[test]
fn a_failed_write_confirms_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().fail_writes();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::NoSocket)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_quiet_until_the_deadline(&mut tester);
    assert_eq!(tester.io_error(), Some(&MockError));
}

/// A request that left and was never acknowledged has an unknown outcome, so an I/O
/// failure confirms it `DoIP_ERROR` (8.2.5), then reports the close.
#[test]
fn a_failed_read_with_a_request_sent_confirms_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.latest().fail_reads();
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Error)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

// --- cancellation ----------------------------------------------------------------------

/// Runs `scenario` once for every number of polls `next_event` can be given before it
/// completes: each time over a fresh connection, with the first `next_event` polled that
/// many times and dropped, and then `check` run on the same tester. `prepare` sets the
/// connection up; reads and writes move one byte per poll, so every byte is a point the
/// future can be dropped at.
fn drop_at_every_poll(
    prepare: impl Fn(&MockStack, &mut ActiveTester<'_>),
    check: impl Fn(&MockStack, &mut ActiveTester<'_>, usize),
) {
    for polls in 0.. {
        let stack = MockStack::new(1);
        let mut tester = active(&stack);
        prepare(&stack, &mut tester);
        let mut buf = [0; N];
        let completed = {
            let mut first = pin!(tester.next_event(&mut buf, None));
            poll_times(first.as_mut(), polls).is_some()
        };
        if completed {
            assert!(polls > 1, "the scenario must have await points to drop at");
            return;
        }
        check(&stack, &mut tester, polls);
    }
}

/// A diagnostic message half-read when `next_event` is dropped is
/// delivered whole by the next call, into whatever buffer that call brings.
#[test]
fn cancelled_mid_frame_resumes_into_a_new_buffer() {
    let _clock = clock();
    let pdu: Vec<u8> = (0..10).collect();
    drop_at_every_poll(
        |stack, _| {
            stack.latest().send(&diagnostic(ENTITY, TESTER, &pdu));
            stack
                .latest()
                .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
        },
        |_, tester, polls| {
            let mut small = [0; 4];
            assert_eq!(
                run(tester.next_event(&mut small, None)).unwrap(),
                ConnectionEvent::IndicationTruncated {
                    sa: ENTITY,
                    ta: TESTER,
                    ta_type: TaType::Physical,
                    pdu: &pdu[..4],
                    length: 10,
                },
                "dropped after {polls} polls"
            );
            let mut buf = [0; 16];
            assert!(
                matches!(
                    run(tester.next_event(&mut buf, None)).unwrap(),
                    ConnectionEvent::Indication {
                        pdu: [0x7E, 0x00],
                        ..
                    }
                ),
                "dropped after {polls} polls"
            );
        },
    );
}

#[test]
fn cancelled_while_answering_an_alive_check_writes_the_answer_once() {
    let _clock = clock();
    drop_at_every_poll(
        |stack, _| {
            stack.latest().send(&alive_check_request());
            stack
                .latest()
                .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
        },
        |stack, tester, polls| {
            let mut buf = [0; 16];
            assert!(
                matches!(
                    run(tester.next_event(&mut buf, None)).unwrap(),
                    ConnectionEvent::Indication { .. }
                ),
                "dropped after {polls} polls"
            );
            assert_eq!(
                stack.latest().take_written(),
                alive_check_response(),
                "dropped after {polls} polls"
            );
        },
    );
}

/// An alive check between a request and its acknowledgement is answered
/// once, and the acknowledgement still confirms the request.
#[test]
fn an_alive_check_between_a_request_and_its_ack_is_answered_once() {
    let _clock = clock();
    drop_at_every_poll(
        |stack, tester| {
            tester.request(ENTITY, TaType::Physical, &PDU).unwrap();
            sent(stack, tester);
            stack.latest().send(&alive_check_request());
            stack.latest().send(&ack(ENTITY, TESTER));
        },
        |stack, tester, polls| {
            let mut buf = [0; 16];
            assert_eq!(
                run(tester.next_event(&mut buf, None)).unwrap(),
                confirm(DoIpResult::Ok),
                "dropped after {polls} polls"
            );
            assert_eq!(
                stack.latest().take_written(),
                alive_check_response(),
                "dropped after {polls} polls"
            );
        },
    );
}

/// A message longer than the tester's buffer, dropped anywhere in it,
/// still leaves the next message whole.
#[test]
fn a_cancelled_oversized_message_leaves_the_stream_in_sync() {
    let _clock = clock();
    let long: Vec<u8> = (0..100).collect();
    drop_at_every_poll(
        |stack, _| {
            stack.latest().send(&diagnostic(ENTITY, TESTER, &long));
            stack
                .latest()
                .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
        },
        |_, tester, polls| {
            let mut buf = [0; 128];
            assert!(
                matches!(
                    run(tester.next_event(&mut buf, None)).unwrap(),
                    ConnectionEvent::IndicationTruncated { length: 100, .. }
                ),
                "dropped after {polls} polls"
            );
            assert!(
                matches!(
                    run(tester.next_event(&mut buf, None)).unwrap(),
                    ConnectionEvent::Indication {
                        pdu: [0x7E, 0x00],
                        ..
                    }
                ),
                "dropped after {polls} polls"
            );
        },
    );
}

/// The acknowledgement timer lives in the tester, not the future: dropping `next_event`
/// while it waits neither restarts nor loses `A_DoIP_Diagnostic_Message`.
#[test]
fn cancelled_while_waiting_for_the_ack_keeps_its_deadline() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];

    for _ in 0..2 {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(Duration::from_millis(1000));
    }

    assert_eq!(
        run(tester.next_event(&mut buf, None)).unwrap(),
        confirm(DoIpResult::TimeoutA)
    );
}

// --- reconnect -------------------------------------------------------------------------

/// ISO 14229-5:2022 REQ 7.8 and REQ 7.10: after the server closes the connection for a
/// session change or a reset, the client performs a new TCP connection and routing
/// activation before continuing.
#[test]
fn reconnecting_after_a_close_opens_a_new_connection_and_activates() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    stack.script_next(&activation_response(0x10));

    reconnect(&mut tester).unwrap();

    assert_eq!(stack.connects(), 2);
    assert_eq!(stack.latest().take_written(), activation_request());
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));
}

#[test]
fn reconnecting_while_connected_gives_up_the_old_connection() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.script_next(&activation_response(0x10));

    reconnect(&mut tester).unwrap();

    assert!(stack.peer(0).is_shut());
    assert!(!stack.peer(1).is_shut());
}

/// A request outstanding when the tester reconnects is confirmed as failed
/// before anything from the new connection, and only once.
#[test]
fn reconnecting_confirms_the_outstanding_request_first() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.script_next(&activation_response(0x10));
    reconnect(&mut tester).unwrap();
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Error)
    );
    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
}

#[test]
fn a_failed_reconnect_leaves_the_tester_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.refuse_next_connect();
    stack.script_next(&activation_response(0x00));
    let mut buf = [0; 16];

    assert_eq!(
        reconnect(&mut tester).unwrap_err(),
        ConnectError::Io(MockError)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );

    assert_eq!(
        reconnect(&mut tester).unwrap_err(),
        ConnectError::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedUnknownSourceAddress
        )
    );
    assert_quiet_until_the_deadline(&mut tester);
    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NotConnected);
}

/// A reconnect dropped before routing is active leaves no connection behind: the old
/// one is reported closed, and the half-activated new one is given up.
#[test]
fn a_dropped_reconnect_leaves_the_tester_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    {
        let mut reconnecting = pin!(tester.reconnect(None));
        assert!(until_stalled(reconnecting.as_mut()).is_none());
        advance(RECONNECT_BACKOFF);
        assert!(until_stalled(reconnecting.as_mut()).is_none());
    }
    let mut buf = [0; 16];

    assert!(stack.peer(1).is_shut());
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_quiet_until_the_deadline(&mut tester);
}

/// Issue #17 item 2: a socket dropped mid-activation may still hold the tester's address
/// at the entity, so the next reconnect backs off from the drop, not from when the
/// dropped socket opened.
#[test]
fn a_reconnect_dropped_mid_activation_still_backs_off_the_next() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    {
        let mut reconnecting = pin!(tester.reconnect(None));
        assert!(until_stalled(reconnecting.as_mut()).is_none());
        advance(RECONNECT_BACKOFF);
        assert!(until_stalled(reconnecting.as_mut()).is_none());
        assert_eq!(stack.connects(), 2);
        advance(Duration::from_secs(1));
    }
    stack.script_next(&activation_response(0x10));
    let mut reconnecting = pin!(tester.reconnect(None));

    assert!(until_stalled(reconnecting.as_mut()).is_none());
    advance(RECONNECT_BACKOFF - Duration::from_millis(1));
    assert!(until_stalled(reconnecting.as_mut()).is_none());
    assert_eq!(stack.connects(), 2, "still backing off");
    advance(Duration::from_millis(1));
    assert!(matches!(
        until_stalled(reconnecting.as_mut()),
        Some(Ok(Reconnection::Connected))
    ));
    assert_eq!(stack.connects(), 3);
}

// --- correlation and the strict timeout ------------------------------------------------

/// Writes everything `request` queued by polling `next_event` until it waits on the
/// entity, then forgets what was written.
fn sent(stack: &MockStack, tester: &mut ActiveTester<'_>) {
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));
    assert!(until_stalled(waiting.as_mut()).is_none());
    stack.latest().take_written();
}

/// ISO 13400-2:2019 Table 12: once `A_DoIP_Diagnostic_Message` has passed, the request
/// is lost, however late the caller comes back. The tester gives the
/// connection up, so neither its acknowledgement nor its response can reach a later
/// request.
#[test]
fn timeout_a_gives_up_the_connection() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.latest().send(&ack(ENTITY, TESTER));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x62, 0xF1, 0x90]));
    advance(Duration::from_millis(2500));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::TimeoutA)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert!(stack.latest().is_aborted());
    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NotConnected);
}

/// ISO 13400-2:2019 Table 12: the timeout is not put off by traffic that keeps the
/// tester busy; it is reported before anything read after it passed.
#[test]
fn timeout_a_is_reported_before_traffic_that_follows_it() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    let mut buf = [0; 16];
    advance(Duration::from_millis(1000));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));

    advance(Duration::from_millis(1000));
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::TimeoutA)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// ISO 13400-2:2019 Table 23: a physical request's acknowledgement comes from the target
/// it was sent to; one from elsewhere is not its confirm.
#[test]
fn a_physical_requests_ack_from_another_source_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().send(&ack(LogicalAddress(0x0002), TESTER));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    assert!(stack.latest().all_read());
    advance(Duration::from_secs(2));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::TimeoutA)))
    );
}

#[test]
fn a_nack_for_another_tester_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack
        .latest()
        .send(&nack(ENTITY, LogicalAddress(0x0E01), 0x03));
    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// A request lost with its connection is owed its confirm; until that is reported, a new
/// request is not accepted.
#[test]
fn a_request_while_a_confirm_is_owed_is_pending() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.script_next(&activation_response(0x10));
    reconnect(&mut tester).unwrap();

    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NoRoom);
}

/// The caller's deadline is kept while an acknowledgement is awaited, even though
/// `A_DoIP_Diagnostic_Message` ends later.
#[test]
fn the_callers_deadline_comes_first_while_an_ack_is_awaited() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(500))));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(500));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

// --- a peer that stops reading ---------------------------------------------------------

/// A request the entity takes none of within `A_DoIP_Diagnostic_Message` of the tester
/// starting to write it is lost; nothing of it was written, so the stream is still in
/// step and the connection is kept.
#[test]
fn a_request_never_begun_times_out_and_keeps_the_connection() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes();
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(Duration::from_millis(2000));
        assert_eq!(
            until_stalled(waiting.as_mut()),
            Some(Ok(confirm(DoIpResult::TimeoutA)))
        );
    }
    assert!(!stack.latest().is_shut());
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
}

/// A request written only in part cannot be withdrawn without breaking the stream, so
/// when it is lost the connection goes with it.
#[test]
fn a_request_stuck_part_written_times_out_and_closes() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes_after(5);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(Duration::from_millis(2000));
        assert_eq!(
            until_stalled(waiting.as_mut()),
            Some(Ok(confirm(DoIpResult::TimeoutA)))
        );
    }
    assert!(stack.latest().is_aborted());
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// `A_DoIP_Diagnostic_Message` measures the entity, so a caller that queues a request and
/// comes back to `next_event` only later still has it sent and confirmed.
#[test]
fn a_request_polled_late_is_still_sent_and_confirmed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    advance(Duration::from_secs(3));
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, None));

    assert!(until_stalled(waiting.as_mut()).is_none());
    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &PDU)
    );
    stack.latest().send(&ack(ENTITY, TESTER));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(confirm(DoIpResult::Ok)))
    );
}

#[test]
fn the_callers_deadline_ends_a_stalled_write() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes();
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(100))));

    assert!(until_stalled(waiting.as_mut()).is_none());
    advance(Duration::from_millis(100));
    assert_eq!(
        until_stalled(waiting.as_mut()),
        Some(Ok(ConnectionEvent::Deadline))
    );
}

/// A write that completes having written nothing is a closed connection.
#[test]
fn a_write_of_nothing_closes_and_confirms_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().write_nothing();
    let _ = request(&mut tester);
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::NoSocket)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert!(stack.latest().is_shut());
    assert!(
        !stack.latest().is_aborted(),
        "the socket ended it; nothing to abort"
    );
}

// --- messages in error -----------------------------------------------------------------

/// ISO 13400-2:2019 Table 16: a tester understands the messages of every edition it
/// defines, ISO/DIS 13400-2:2010's and ISO 13400-2:2012's as well as its own.
#[test]
fn a_message_of_an_earlier_edition_is_indicated() {
    let _clock = clock();
    for version in [0x01, 0x02] {
        let stack = MockStack::new(usize::MAX);
        let mut tester = active(&stack);
        stack.latest().send(&versioned(
            diagnostic(ENTITY, TESTER, &[0x7E, 0x00]),
            version,
        ));
        let mut buf = [0; 16];

        assert!(
            matches!(
                next(&mut tester, &mut buf).unwrap(),
                ConnectionEvent::Indication { .. }
            ),
            "{version:#04X}"
        );
    }
}

/// ISO 13400-2:2019 Table 16 and Figure 16: any other protocol version is an incorrect
/// pattern, after which the stream cannot be trusted, so the tester closes.
#[test]
fn a_message_of_any_other_version_closes_the_connection() {
    let _clock = clock();
    for version in [0x00, 0x04, 0xFE, 0xFF] {
        let stack = MockStack::new(usize::MAX);
        let mut tester = active(&stack);
        stack.latest().send(&versioned(
            diagnostic(ENTITY, TESTER, &[0x7E, 0x00]),
            version,
        ));
        let mut buf = [0; 16];

        assert_eq!(
            next(&mut tester, &mut buf).unwrap(),
            ConnectionEvent::Closed,
            "{version:#04X}"
        );
        assert!(stack.latest().is_aborted(), "{version:#04X}");
    }
}

/// ISO 13400-2:2019 Table 21: the user data is mandatory, so a diagnostic message of its
/// addresses alone is in error, and 8.3.3 raises no indication for it.
#[test]
fn a_diagnostic_message_without_user_data_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&raw(0x8001, &[0x00, 0x01, 0x0E, 0x00]));
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

/// ISO 13400-2:2019 9.3 (`0x04`): the payload length must match its type, so a message
/// whose length is wrong for its type is ignored, not acted on.
#[test]
fn messages_of_the_wrong_length_for_their_type_are_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().send(&raw(0x0000, &[0x02, 0x00, 0x00]));
    stack.latest().send(&raw(0x0007, &[0x00, 0x00]));
    stack.latest().send(&raw(0x8002, &[0x00, 0x01, 0x0E, 0x00]));
    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
    assert_eq!(
        stack.latest().take_written(),
        diagnostic(TESTER, ENTITY, &PDU)
    );
}

/// ISO 13400-2:2019 Table 24: `0x00` is the only positive acknowledgement code. One with
/// a reserved code is still the entity's acknowledgement, confirmed as the error 8.2.5
/// gives what no other result names, as a reserved negative code is.
#[test]
fn a_positive_ack_with_a_reserved_code_confirms_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack
        .latest()
        .send(&raw(0x8002, &[0x00, 0x01, 0x0E, 0x00, 0x7F]));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Error)
    );
}

/// ISO 13400-2:2019 Table 21: the target is the receiver, so a message for another tester
/// is not this one's to indicate.
#[test]
fn a_diagnostic_message_for_another_tester_is_ignored() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&diagnostic(ENTITY, LogicalAddress(0x0E01), &[0x7E, 0x01]));
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

// --- failures after which the entity closes --------------------------------------------

/// ISO 13400-2:2019 REQ 7.DoIP-070: an entity rejecting a source address closes the
/// socket, so the tester confirms the request and gives the connection up itself.
#[test]
fn an_invalid_source_address_nack_confirms_then_closes() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.latest().send(&nack(ENTITY, TESTER, 0x02));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::InvalidSa)
    );
    assert!(stack.latest().is_aborted());
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// ISO 13400-2:2019 Table 19: after an incorrect pattern (`0x00`) or an invalid payload
/// length (`0x04`) the entity closes the socket; after the other codes it does not.
#[test]
fn a_header_nack_on_which_the_entity_closes_closes_the_tester() {
    let _clock = clock();
    for (code, closes) in [
        (0x00, true),
        (0x01, false),
        (0x02, false),
        (0x03, false),
        (0x04, true),
    ] {
        let stack = MockStack::new(usize::MAX);
        let mut tester = active(&stack);
        request(&mut tester).unwrap();
        stack.latest().send(&header_nack(code));
        let mut buf = [0; 16];
        next(&mut tester, &mut buf).unwrap();

        assert_eq!(stack.latest().is_aborted(), closes, "{code:#04X}");
        let after = run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap();
        let expected = if closes {
            ConnectionEvent::Closed
        } else {
            ConnectionEvent::Deadline
        };
        assert_eq!(after, expected, "{code:#04X}");
    }
}

// --- aborting, not only dropping -------------------------------------------------------

/// A connection the tester ends on its own initiative is aborted, not left to an orderly
/// close a dead or confused peer may never complete.
#[test]
fn the_tester_aborts_what_it_gives_up() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&activation_response(0x00));
    connect(&stack).unwrap_err();
    assert!(stack.latest().is_aborted());

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
    assert!(stack.latest().is_aborted());
}

/// A reconnect dropped at any await after it started leaves the tester reporting the old
/// connection closed; one that completed leaves it connected.
#[test]
fn a_reconnect_dropped_at_any_await_leaves_a_consistent_tester() {
    let _clock = clock();
    for polls in 0.. {
        let stack = MockStack::new(1);
        let mut tester = active(&stack);
        stack.script_next(&activation_response(0x10));
        let completed = {
            let mut reconnecting = pin!(tester.reconnect(None));
            (0..polls).any(|_| {
                let done = poll_times(reconnecting.as_mut(), 1).is_some();
                advance(RECONNECT_BACKOFF);
                done
            })
        };
        let mut buf = [0; 16];
        let first = run(tester.next_event(&mut buf, Some(Timestamp(0))));
        if polls == 0 {
            assert_eq!(first.unwrap(), ConnectionEvent::Deadline, "never polled");
            continue;
        }
        if completed {
            assert_eq!(first.unwrap(), ConnectionEvent::Deadline);
            assert!(polls > 1, "reconnect must have await points to drop at");
            return;
        }
        assert_eq!(
            first.unwrap(),
            ConnectionEvent::Closed,
            "dropped after {polls} polls"
        );
        assert_eq!(
            run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
            ConnectionEvent::Deadline,
            "dropped after {polls} polls"
        );
        assert!(stack.peer(0).is_shut(), "dropped after {polls} polls");
    }
}

// --- routing activation responses in error ---------------------------------------------

/// ISO 13400-2:2019 Table 48: a response may carry the OEM-specific field.
#[test]
fn an_activation_response_with_its_oem_field_activates() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut response = raw(0x0006, &[0x0E, 0x00, 0x00, 0x01, 0x10, 0, 0, 0, 0]);
    response.extend([0xAA; 4]);
    response[7] = 13;
    stack.script_next(&response);

    assert!(connect(&stack).is_ok());
}

/// ISO 13400-2:2019 Table 48 and Table 16: a response of the wrong length, or in a
/// protocol version the tester does not speak, is not an answer; the tester gives up.
#[test]
fn an_activation_response_in_error_fails_at_once() {
    let _clock = clock();
    for response in [
        raw(0x0006, &[0x0E, 0x00, 0x00, 0x01, 0x10]),
        raw(
            0x0006,
            &[0x0E, 0x00, 0x00, 0x01, 0x10, 0, 0, 0, 0, 0xAA, 0xAA],
        ),
        versioned(activation_response(0x10), 0xFF),
        versioned(activation_response(0x10), 0x04),
    ] {
        let stack = MockStack::new(usize::MAX);
        stack.script_next(&response);

        assert_eq!(
            connect(&stack).unwrap_err(),
            ConnectError::InvalidMessage,
            "{response:02X?}"
        );
        assert!(stack.latest().is_aborted(), "{response:02X?}");
    }
}

/// A message of a payload type this crate does not model, longer than the tester's
/// buffer or the caller's, is reported truncated with its whole length, and the stream
/// stays in sync.
#[test]
fn an_unmodelled_message_too_long_to_hold_is_reported_truncated() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let long: Vec<u8> = (0..100).collect();
    stack.latest().send(&raw(0x8004, &long));
    stack.latest().send(&raw(0x8004, &[1, 2, 3, 4, 5, 6]));
    let mut buf = [0; 128];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::UnmodelledTruncated {
            payload_type: 0x8004,
            data: &long[..N - 8],
            length: 100,
        }
    );
    let mut small = [0; 4];
    assert_eq!(
        next(&mut tester, &mut small).unwrap(),
        ConnectionEvent::UnmodelledTruncated {
            payload_type: 0x8004,
            data: &[1, 2, 3, 4],
            length: 6,
        }
    );
}

// --- round two: what the entity may send that the tester must not misread --------------

/// ISO 13400-2:2019 REQ 7.DoIP-040: a generic header NACK reports an error in a message
/// the tester sent, which once it has answered an alive check need not be its request;
/// the request then waits for its own acknowledgement.
#[test]
fn a_header_nack_after_an_alive_check_answer_is_not_the_requests() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.latest().send(&alive_check_request());
    stack.latest().send(&header_nack(0x03));
    stack.latest().send(&ack(ENTITY, TESTER));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
    assert_eq!(stack.latest().take_written(), alive_check_response());
}

/// The caller's deadline is kept even while messages the tester ignores keep arriving.
#[test]
fn the_callers_deadline_is_kept_while_ignored_messages_flood_in() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    for _ in 0..50 {
        stack
            .latest()
            .send(&diagnostic(ENTITY, LogicalAddress(0x0E01), &[0x7E, 0x00]));
    }
    let mut buf = [0; 16];

    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert!(!stack.latest().all_read());
}

/// ISO 13400-2:2019 Table 48: a routing activation response longer than any it may be is
/// in error even where it does not fit the tester's buffer, and fails at once.
#[test]
fn an_oversized_activation_response_fails_at_once() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&raw(
        0x0006,
        &[0x0E, 0x00, 0x00, 0x01, 0x10, 0, 0, 0, 0, 1, 2, 3, 4, 5],
    ));

    let result = run(Tester::<_, MIN_N>::connect(&stack, REMOTE, sa()));

    assert_eq!(result.unwrap_err(), ConnectError::InvalidMessage);
    assert!(stack.latest().is_aborted());
}

/// ISO 13400-2:2019 Table 16: a protocol version the tester does not speak fails routing
/// activation even on a message too long for the tester's buffer.
#[test]
fn an_oversized_message_in_another_version_fails_activation() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    stack.script_next(&versioned(diagnostic(ENTITY, TESTER, &[0; 30]), 0x04));

    let result = run(Tester::<_, MIN_N>::connect(&stack, REMOTE, sa()));

    assert_eq!(result.unwrap_err(), ConnectError::InvalidMessage);
}

/// Bytes the old connection left half a message of do not run into the new one's.
#[test]
fn a_partial_message_does_not_survive_a_reconnect() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00])[..6]);
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
    }
    stack.script_next(&activation_response(0x10));
    reconnect(&mut tester).unwrap();
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x62, 0xF1, 0x90]));

    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication {
            pdu: [0x62, 0xF1, 0x90],
            ..
        }
    ));
}

// --- the caller's deadline -------------------------------------------------------------

/// Once the caller's deadline has passed, a message still arriving is read no further
/// than one read: an entity streaming a body larger than the tester's buffer does not
/// hold the deadline off.
#[test]
fn a_message_still_arriving_does_not_hold_off_a_passed_deadline() {
    let _clock = clock();
    let stack = MockStack::new(N);
    let mut tester = active(&stack);
    stack.latest().send(&raw(0x0004, &vec![0; 1 << 20]));
    let mut buf = [0; 16];

    for _ in 0..3 {
        let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(0))));
        assert_eq!(
            poll_times(waiting.as_mut(), 50),
            Some(Ok(ConnectionEvent::Deadline))
        );
    }
    assert!(!stack.latest().all_read());
}

/// The same for a message within the buffer, arriving a byte at a time.
#[test]
fn a_message_trickling_in_does_not_hold_off_a_passed_deadline() {
    let _clock = clock();
    let stack = MockStack::new(1);
    let mut tester = active(&stack);
    stack.latest().send(&diagnostic(ENTITY, TESTER, &[0; 40]));
    let mut buf = [0; 64];

    let mut waiting = pin!(tester.next_event(&mut buf, Some(Timestamp(0))));
    assert_eq!(
        poll_times(waiting.as_mut(), 50),
        Some(Ok(ConnectionEvent::Deadline))
    );
    assert!(!stack.latest().all_read());
}

/// A passed deadline still reads what is ready: the message that has arrived is
/// delivered rather than reported as the deadline.
#[test]
fn a_passed_deadline_still_delivers_what_has_arrived() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack
        .latest()
        .send(&diagnostic(ENTITY, TESTER, &[0x7E, 0x00]));
    let mut buf = [0; 16];

    assert!(matches!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Indication {
            pdu: [0x7E, 0x00],
            ..
        }
    ));
}

/// ISO 13400-2:2019 REQ 3.DoIP-092: the entity closes a socket whose tester does not
/// answer its alive check within `T_TCP_Alive_Check`. An answer the tester has read the
/// request for is written before the deadline is reported, not when the caller next
/// polls.
#[test]
fn an_alive_check_is_answered_before_the_deadline_is_reported() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&alive_check_request());
    let mut buf = [0; 16];

    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(stack.latest().take_written(), alive_check_response());
}

// --- which acknowledgement is the request's --------------------------------------------

/// An acknowledgement can only follow the request it acknowledges, so one read before
/// the request's first byte was written is not the request's, even if the tester only
/// takes it from its buffer afterwards.
#[test]
fn a_header_nack_read_before_the_request_is_not_its() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&alive_check_request());
    advance(Duration::from_secs(3));
    let mut buf = [0; 16];
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    advance(Duration::from_secs(3));
    let mut both = diagnostic(ENTITY, TESTER, &[0x7E, 0x00]);
    both.extend(header_nack(0x03));
    stack.latest().send(&both);
    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));
    assert!(
        stack.latest().all_read(),
        "the NACK waits in the tester's buffer"
    );

    request(&mut tester).unwrap();
    stack.latest().send(&ack(ENTITY, TESTER));

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// The same for a positive acknowledgement.
#[test]
fn an_ack_read_before_the_request_is_not_its() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    let mut both = diagnostic(ENTITY, TESTER, &[0x7E, 0x00]);
    both.extend(ack(ENTITY, TESTER));
    stack.latest().send(&both);
    let mut buf = [0; 16];
    assert!(matches!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Indication { .. }
    ));

    request(&mut tester).unwrap();

    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    stack.latest().send(&nack(ENTITY, TESTER, 0x03));
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::UnknownTa)
    );
}

/// REQ 7.DoIP-040: a generic header NACK is about a message the tester sent. One that
/// arrives within `A_DoIP_Diagnostic_Message` of an alive check response written just
/// before the request may be about that response, so it is not the request's.
#[test]
fn a_header_nack_soon_after_an_alive_check_answer_before_the_request_is_not_its() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&alive_check_request());
    let mut buf = [0; 16];
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(stack.latest().take_written(), alive_check_response());
    advance(Duration::from_millis(1_900));

    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.latest().send(&header_nack(0x03));
    stack.latest().send(&ack(ENTITY, TESTER));

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// An alive check response still waiting to be written when the request starts goes out
/// ahead of it, so a header NACK after both may be about either.
#[test]
fn a_header_nack_after_an_alive_check_answer_written_ahead_of_the_request_is_not_its() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes_after(0);
    stack.latest().send(&alive_check_request());
    let mut buf = [0; 16];
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(stack.latest().take_written(), []);
    advance(Duration::from_secs(3));

    request(&mut tester).unwrap();
    stack.latest().resume_writes();
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
    }
    let mut written = alive_check_response();
    written.extend(diagnostic(TESTER, ENTITY, &PDU));
    assert_eq!(
        stack.latest().take_written(),
        written,
        "the answer goes first"
    );
    stack.latest().send(&header_nack(0x03));
    stack.latest().send(&ack(ENTITY, TESTER));

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Ok)
    );
}

/// An alive check response written longer than `A_DoIP_Diagnostic_Message` before the
/// request is past anything the entity would still answer, so a header NACK is the
/// request's.
#[test]
fn a_header_nack_long_after_an_alive_check_answer_is_the_requests() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&alive_check_request());
    let mut buf = [0; 16];
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    advance(Duration::from_secs(2));

    request(&mut tester).unwrap();
    sent(&stack, &mut tester);
    stack.latest().send(&header_nack(0x03));

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::OutOfMemory)
    );
}

// --- writing a request -----------------------------------------------------------------

/// A request one byte of which has been written cannot be withdrawn without putting the
/// stream out of step, so its `TimeoutA` gives the connection up.
#[test]
fn a_request_with_one_byte_written_is_not_withdrawn() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes_after(1);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(ACK_TIMEOUT);
        assert_eq!(
            until_stalled(waiting.as_mut()),
            Some(Ok(confirm(DoIpResult::TimeoutA)))
        );
    }
    assert!(stack.latest().is_aborted());
}

/// A withdrawn request is never written, even once the entity reads again.
#[test]
fn a_withdrawn_request_is_never_written() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_writes_after(0);
    request(&mut tester).unwrap();
    let mut buf = [0; 16];
    {
        let mut waiting = pin!(tester.next_event(&mut buf, None));
        assert!(until_stalled(waiting.as_mut()).is_none());
        advance(ACK_TIMEOUT);
        assert_eq!(
            until_stalled(waiting.as_mut()),
            Some(Ok(confirm(DoIpResult::TimeoutA)))
        );
    }
    stack.latest().resume_writes();

    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(stack.latest().take_written(), []);
}

/// A request queued on the old connection is not written on the new one: routing
/// activation is the first thing a new connection carries.
#[test]
fn a_reconnect_carries_only_the_activation_request() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    stack.script_next(&activation_response(0x10));
    reconnect(&mut tester).unwrap();
    let mut buf = [0; 16];

    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        confirm(DoIpResult::NoSocket)
    );
    assert_eq!(
        run(tester.next_event(&mut buf, Some(Timestamp(0)))).unwrap(),
        ConnectionEvent::Deadline
    );
    assert_eq!(stack.latest().take_written(), activation_request());
}

/// A request being written when `next_event` is dropped is written exactly once, by
/// the calls that follow.
#[test]
fn cancelled_mid_write_writes_the_request_once() {
    let _clock = clock();
    drop_at_every_poll(
        |stack, tester| {
            request(tester).unwrap();
            stack.latest().send(&ack(ENTITY, TESTER));
        },
        |stack, tester, polls| {
            let mut buf = [0; 16];
            assert_eq!(
                run(tester.next_event(&mut buf, None)).unwrap(),
                confirm(DoIpResult::Ok),
                "dropped after {polls} polls"
            );
            assert_eq!(
                stack.latest().take_written(),
                diagnostic(TESTER, ENTITY, &PDU),
                "dropped after {polls} polls"
            );
        },
    );
}

// --- messages during routing activation ------------------------------------------------

/// Routing activation ignores an alive check request of the wrong length, as
/// `next_event` does, rather than answering it.
#[test]
fn activation_ignores_an_alive_check_request_of_the_wrong_length() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut wrong = raw(0x0007, &[0]);
    wrong.extend(activation_response(0x10));
    stack.script_next(&wrong);

    connect(&stack).unwrap();

    assert_eq!(stack.latest().take_written(), activation_request());
}

/// Routing activation ignores a generic header NACK of the wrong length, a message in
/// error (8.3.3), rather than failing on it.
#[test]
fn activation_ignores_a_header_nack_of_the_wrong_length() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut wrong = raw(0x0000, &[0, 0]);
    wrong.extend(activation_response(0x10));
    stack.script_next(&wrong);

    assert!(connect(&stack).is_ok());
}

/// ISO 13400-2:2019 Table 17: a payload type reserved for the vehicle manufacturer is
/// reported, as one reserved by the document is.
#[test]
fn a_manufacturer_payload_type_is_reported() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().send(&raw(0xF000, &[1, 2]));
    let mut buf = [0; 16];

    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Unmodelled {
            payload_type: 0xF000,
            data: &[1, 2],
        }
    );
}

// --- close ----------------------------------------------------------------------------

/// `close` ends the connection gracefully rather than aborting or merely dropping it, and
/// leaves the tester closed: `Closed` once, then quiet, and `NotConnected` for a request.
#[test]
fn close_shuts_the_connection_gracefully_and_leaves_it_closed() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);

    run(tester.close()).unwrap();

    assert!(stack.latest().is_closed());
    assert!(!stack.latest().is_aborted());
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_quiet_until_the_deadline(&mut tester);
    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NotConnected);
}

/// A request unconfirmed at the close still gets its one confirm, from the next
/// `next_event`, before `Closed`: `DoIP_NO_SOCKET` if none of it left.
#[test]
fn close_with_a_request_unwritten_confirms_no_socket() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();

    run(tester.close()).unwrap();

    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::NoSocket)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    assert_eq!(stack.latest().take_written(), []);
}

/// `DoIP_ERROR` if it left and was never acknowledged.
#[test]
fn close_with_a_request_sent_confirms_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    request(&mut tester).unwrap();
    sent(&stack, &mut tester);

    run(tester.close()).unwrap();

    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        confirm(DoIpResult::Error)
    );
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// A close dropped before it completes drops the socket, and the tester is closed all
/// the same.
#[test]
fn a_dropped_close_drops_the_socket() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().stall_closes();
    {
        let mut closing = pin!(tester.close());
        assert!(until_stalled(closing.as_mut()).is_none());
    }

    assert!(stack.latest().is_shut());
    assert!(!stack.latest().is_closed());
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
}

/// Closing twice, or with no connection, does nothing: no second `Closed` follows.
#[test]
fn closing_a_closed_tester_does_nothing() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );

    run(tester.close()).unwrap();
    run(tester.close()).unwrap();

    assert!(!stack.latest().is_closed());
    assert_quiet_until_the_deadline(&mut tester);
}

/// A closed tester is revived by a reconnect, after the back-off from the close.
#[test]
fn a_reconnect_revives_a_closed_tester() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    run(tester.close()).unwrap();
    stack.script_next(&activation_response(0x10));

    reconnect(&mut tester).unwrap();

    assert_eq!(stack.connects(), 2);
    request(&mut tester).unwrap();
}

// --- reconnecting -------------------------------------------------------------------

/// A reconnect given a deadline stops there, during its back-off or its connect, and
/// leaves the tester closed with the back-off still running from the loss, so a later
/// reconnect finishes it rather than starting it again.
#[test]
fn a_reconnect_stops_at_its_deadline_and_a_later_one_connects() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    run(tester.close()).unwrap();
    stack.script_next(&activation_response(0x10));

    let deadline = tester.now().after(100);
    {
        let mut reconnecting = pin!(tester.reconnect(Some(deadline)));
        assert!(until_stalled(reconnecting.as_mut()).is_none());
        advance(Duration::from_millis(100));
        assert_eq!(
            until_stalled(reconnecting.as_mut()),
            Some(Ok(Reconnection::Deadline))
        );
    }
    assert_eq!(stack.connects(), 1);
    assert_eq!(request(&mut tester).unwrap_err(), Refusal::NotConnected);

    let mut reconnecting = pin!(tester.reconnect(None));
    assert!(until_stalled(reconnecting.as_mut()).is_none());
    advance(RECONNECT_BACKOFF - Duration::from_millis(100));
    assert_eq!(run(reconnecting), Ok(Reconnection::Connected));
    assert_eq!(stack.connects(), 2);
}

/// Issue #17 item 2: an entity holds the tester's address for a while after its socket
/// closes and refuses a second activation for it meanwhile, so a reconnect gives the
/// old connection up first, backs off, and only then connects.
#[test]
fn a_reconnect_drops_the_old_connection_then_backs_off_then_connects() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.script_next(&activation_response(0x10));
    let mut reconnecting = pin!(tester.reconnect(None));

    assert!(until_stalled(reconnecting.as_mut()).is_none());
    assert!(stack.peer(0).is_shut(), "the old connection goes first");
    assert_eq!(stack.connects(), 1);
    advance(RECONNECT_BACKOFF - Duration::from_millis(1));
    assert!(until_stalled(reconnecting.as_mut()).is_none());
    assert_eq!(stack.connects(), 1, "still backing off");
    advance(Duration::from_millis(1));

    assert!(matches!(
        until_stalled(reconnecting.as_mut()),
        Some(Ok(Reconnection::Connected))
    ));
    assert_eq!(stack.connects(), 2);
}

/// An entity that frees a tester's address sooner, or later, than the sensor the default
/// was measured on gets a back-off of its own.
#[test]
fn a_reconnect_backs_off_for_as_long_as_the_tester_was_given() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let backoff = Duration::from_millis(500);
    let mut tester = active(&stack).with_reconnect_backoff(backoff);
    stack.script_next(&activation_response(0x10));
    let mut reconnecting = pin!(tester.reconnect(None));

    assert!(until_stalled(reconnecting.as_mut()).is_none());
    advance(backoff - Duration::from_millis(1));
    assert!(until_stalled(reconnecting.as_mut()).is_none());
    assert_eq!(stack.connects(), 1, "still backing off");
    advance(Duration::from_millis(1));

    assert!(matches!(
        until_stalled(reconnecting.as_mut()),
        Some(Ok(Reconnection::Connected))
    ));
    assert_eq!(stack.connects(), 2);
}

/// The back-off runs from when the old connection was lost: a reconnect made long after
/// does not wait again.
#[test]
fn a_reconnect_long_after_the_close_does_not_back_off() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().eof();
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    advance(RECONNECT_BACKOFF);
    stack.script_next(&activation_response(0x10));

    run(tester.reconnect(None)).unwrap();

    assert_eq!(stack.connects(), 2);
}

/// A connection given up after a refused activation is one the entity may still hold,
/// so the next reconnect backs off from it too.
#[test]
fn a_reconnect_after_a_refused_activation_backs_off_again() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.script_next(&activation_response(0x03));
    assert_eq!(
        reconnect(&mut tester).unwrap_err(),
        ConnectError::RoutingActivationDenied(
            RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered
        )
    );
    stack.script_next(&activation_response(0x10));
    let mut reconnecting = pin!(tester.reconnect(None));

    assert!(until_stalled(reconnecting.as_mut()).is_none());
    assert_eq!(stack.connects(), 2);
    advance(RECONNECT_BACKOFF);
    assert!(matches!(
        until_stalled(reconnecting.as_mut()),
        Some(Ok(Reconnection::Connected))
    ));
    assert_eq!(stack.connects(), 3);
}

/// A successful reconnect forgets the error that ended the old connection.
#[test]
fn a_reconnect_clears_the_io_error() {
    let _clock = clock();
    let stack = MockStack::new(usize::MAX);
    let mut tester = active(&stack);
    stack.latest().fail_reads();
    let mut buf = [0; 16];
    assert_eq!(
        next(&mut tester, &mut buf).unwrap(),
        ConnectionEvent::Closed
    );
    stack.script_next(&activation_response(0x10));

    reconnect(&mut tester).unwrap();

    assert_eq!(tester.io_error(), None);
}
