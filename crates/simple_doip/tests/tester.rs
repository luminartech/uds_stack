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
use simple_doip::LogicalAddress;
use simple_doip::messages::{NackCode, RoutingActivationResponseCode};
use simple_doip::service::NotATesterAddress;
use simple_doip::tester::{Error, Tester};
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
