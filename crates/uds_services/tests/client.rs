//! The client end to end, over a scripted transport playing the servers.

#![allow(
    clippy::panic,
    reason = "test harness: an unexpected outcome fails the test with what it was"
)]

mod common;

use common::{Script, Step, block_on};
use core::future::Future;
use core::task::Poll;
use uds_services::{
    Address, Ai, Answer, ClientError, ClientTiming, DataIdentifier, DiagnosticSessionType,
    FunctionalKeepAlive, KeepAlive, MalformedResponse, Mtype, NegativeResponseCode,
    PhysicalKeepAlive, RecordError, Reloads, Response, SResult, SessionTiming, TaType,
    Timestamp, uds_client,
};
use uds_session::TransportError;

const TESTER: Address = Address(0x0E80);
const ECU: Address = Address(0x0010);
const OTHER_ECU: Address = Address(0x0011);
const GROUP: Address = Address(0xE400);
const S3_CLIENT: u32 = 2_000;
const TIMING: ClientTiming = ClientTiming::new(10, 10, 0);

/// The request to `ecu`, physically addressed.
const fn to(ecu: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: TESTER,
        ta: ecu,
        ta_type: TaType::Physical,
    }
}

/// The request to the functional group.
const FUNCTIONAL: Ai = Ai {
    mtype: Mtype::Diag,
    sa: TESTER,
    ta: GROUP,
    ta_type: TaType::Functional,
};

/// A response from `ecu`.
const fn from(ecu: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: ecu,
        ta: TESTER,
        ta_type: TaType::Physical,
    }
}

const FAILED: SResult = SResult::Transport(TransportError(1));

/// One identifier vocabulary per client below: `uds_client!` implements its
/// `ClientSet` on the enumeration, so each mode needs its own.
macro_rules! vocabulary {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            Speed,
            Odometer,
        }

        impl DataIdentifier for $name {
            const MAX_RECORD_LEN: usize = 1;
            fn as_u16(self) -> u16 {
                match self {
                    Self::Speed => 0xF40D,
                    Self::Odometer => 0xF1A0,
                }
            }
            fn from_u16(v: u16) -> Option<Self> {
                match v {
                    0xF40D => Some(Self::Speed),
                    0xF1A0 => Some(Self::Odometer),
                    _ => None,
                }
            }
            fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
                buf.split_at_checked(1).ok_or(RecordError::Short)
            }
        }
    };
}

vocabulary!(Did);
vocabulary!(FunctionalDid);
vocabulary!(WideDid);
vocabulary!(WideFunctionalDid);

uds_client! {
    Did;
    transport = Script,
    max_dids_per_request = 2,
    physical = 2,
    functional = 1,
    responders = 1,
    keep_alive = PhysicalKeepAlive,
    client = Tester,
}

uds_client! {
    FunctionalDid;
    transport = Script,
    max_dids_per_request = 2,
    physical = 1,
    functional = 1,
    responders = 2,
    keep_alive = FunctionalKeepAlive,
    client = FunctionalTester,
}

uds_client! {
    WideDid;
    transport = Script,
    max_dids_per_request = 2,
    physical = 3,
    functional = 1,
    responders = 1,
    keep_alive = PhysicalKeepAlive,
    client = WideTester,
}

uds_client! {
    WideFunctionalDid;
    transport = Script,
    max_dids_per_request = 2,
    physical = 2,
    functional = 1,
    responders = 2,
    keep_alive = FunctionalKeepAlive,
    client = WideFunctionalTester,
}

fn tester(steps: &[Step]) -> Tester {
    Tester::new(
        Script::new(steps),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    )
}

const READ: &[u8] = &[0x22, 0xF4, 0x0D];
const SPEED: &[u8] = &[0x62, 0xF4, 0x0D, 0x40];
const ENTER_EXTENDED: &[u8] = &[0x10, 0x03];
/// `P2Server_max` 50 ms, `P2*Server_max` 0x01F4 — five seconds in Table 29's 10 ms units.
const EXTENDED: &[u8] = &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4];
const KEEP_ALIVE: &[u8] = &[0x3E, 0x80];

/// The speed record of a positive read, or a panic naming what came instead.
fn speed(
    response: Result<Response<uds_services::Records<'_, Did>>, ClientError<()>>,
) -> u8 {
    match response {
        Ok(Response::Positive(mut records)) => match records.next() {
            Some((Did::Speed, [value])) => *value,
            other => panic!("expected one speed record, got {other:?}"),
        },
        other => panic!("expected a positive response, got {other:?}"),
    }
}

/// Poll `f` once, as an executor would before dropping it.
fn poll_once<F: Future>(f: F) -> Poll<F::Output> {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    core::pin::pin!(f).poll(&mut cx)
}

/// ``UDSSVC_ARCH_0020`` — a read is encoded, exchanged and its records decoded against
/// the application's own identifiers.
#[test]
fn a_physical_read_is_answered() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), SPEED),
    ]);
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().addressed(0), (Some(to(ECU)), READ));
    assert!(t.transport().finished());
}

/// ``UDSSVC_ARCH_0020`` — several identifiers go in one request, big-endian, and come
/// back as one walk.
#[test]
fn two_identifiers_go_in_one_request() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x40, 0xF1, 0xA0, 0x07]),
    ]);
    let response = block_on(t.read_data_by_identifier(ECU, &[Did::Speed, Did::Odometer]));
    let Ok(Response::Positive(records)) = response else {
        panic!("expected a positive response, got {response:?}");
    };
    let pairs: [Option<(Did, &[u8])>; 3] = {
        let mut walk = records;
        [walk.next(), walk.next(), walk.next()]
    };
    assert_eq!(
        pairs,
        [
            Some((Did::Speed, &[0x40][..])),
            Some((Did::Odometer, &[0x07][..])),
            None
        ]
    );
    assert_eq!(t.transport().sent_bytes(0), &[0x22, 0xF4, 0x0D, 0xF1, 0xA0]);
}

/// ``UDSSVC_ARCH_0021`` — a negative response is a response, not an error.
#[test]
fn a_negative_response_is_a_response() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x7F, 0x22, 0x31]),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).ok(),
        Some(Response::Negative(NegativeResponseCode::RequestOutOfRange))
    );
}

/// ``UDSS_LLR_0144`` — a response-pending message is not the answer: it moves the wait to
/// the enhanced window, and the answer after it is.
#[test]
fn a_response_pending_then_the_answer() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x7F, 0x22, 0x78]),
        Step::At(1_000), // past the default window, inside the enhanced one
        Step::Ind(from(ECU), SPEED),
    ]);
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().deadlines, 0);
}

/// ``UDSSVC_ARCH_0023``, ISO 14229-2:2021 9.7 Table 9 — an unanswered request is sent
/// three times in all, and then the timeout is reported as the fault it is.
#[test]
fn an_unanswered_request_is_repeated_twice_then_times_out() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(100),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(200),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(300),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::Timeout)
    );
    let s = t.transport();
    assert_eq!(s.sent_count, 3);
    assert_eq!((s.sent_bytes(1), s.sent_bytes(2)), (READ, READ));
    assert_eq!(s.deadlines, 3);
}

/// ISO 14229-2:2021 Table 9 and ``UDSS_LLR_0171`` — a failed transmission is repeated too,
/// each repeat held until the spacing timer the failure started has run out, not reported.
#[test]
fn a_failed_transmission_is_repeated_after_the_spacing_then_not_sent() {
    let mut t = tester(&[
        Step::Conf(to(ECU), FAILED),
        Step::At(10),
        Step::Conf(to(ECU), FAILED),
        Step::At(20),
        Step::Conf(to(ECU), FAILED),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::NotSent)
    );
    let s = t.transport();
    assert_eq!(s.sent_count, 3);
    assert_eq!(s.sent(1).map(|x| x.at), Some(10));
    assert_eq!(s.sent(2).map(|x| x.at), Some(20));
}

/// A connection closing mid-exchange ends it with an outcome rather than a wait for
/// `tP_Client`; the next call runs over whatever connection the transport has by then.
#[test]
fn a_close_mid_exchange_is_reported_and_the_next_call_runs() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Close(ECU),
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), SPEED),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::Closed)
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().deadlines, 0);
}

/// A response longer than the buffer folded from the vocabulary is the server's
/// disagreement with it: `Malformed`, never walked as records.
#[test]
fn a_response_longer_than_the_buffer_is_malformed() {
    let long: &[u8] = &[0x62, 0xF4, 0x0D, 0x40, 0xF4, 0x0D, 0x41, 0xF4, 0x0D, 0x42];
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::TooLong(from(ECU), long),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).ok(),
        Some(Response::Malformed(MalformedResponse::Overlong {
            declared: Some(long.len())
        }))
    );
}

/// A request naming no identifier, or more than `max_dids_per_request`, is refused
/// before anything reaches the session layer or the transport.
#[test]
fn a_read_of_nothing_or_too_much_sends_nothing() {
    let mut t = tester(&[]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[])).err(),
        Some(ClientError::Request)
    );
    let three = [Did::Speed, Did::Odometer, Did::Speed];
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &three)).err(),
        Some(ClientError::Request)
    );
    assert_eq!(t.transport().sent_count, 0);
}

/// ``UDSS_LLR_0071`` — a late reply to an earlier request for another service echoes
/// that service, so it is unsolicited and is not taken for this request's answer.
#[test]
fn a_stale_response_to_another_service_is_not_the_answer() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), EXTENDED),
        Step::Ind(from(ECU), &[0x7F, 0x10, 0x22]),
        Step::Ind(from(ECU), SPEED),
    ]);
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
}

/// A response from another server is not this request's answer.
#[test]
fn a_response_from_another_server_is_not_the_answer() {
    let mut t = tester(&[
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), &[0x62, 0xF4, 0x0D, 0x39]),
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), &[0x62, 0xF4, 0x0D, 0x41]),
        Step::Ind(from(ECU), SPEED),
    ]);
    // The other server's channel is open, so its message is indicated, not dropped.
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(OTHER_ECU, &[Did::Speed])
        )),
        0x39
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
}

/// Cancel safety: a read dropped before its confirmation leaves its exchange recorded,
/// and the next read resets that channel (``UDSS_LLR_0180``), waits for the abandoned
/// transmission's confirmation (``UDSS_LLR_0061``), and is answered.
#[test]
fn a_dropped_read_is_reset_by_the_next() {
    let mut t = tester(&[
        Step::Pend,
        Step::Conf(to(ECU), SResult::Ok), // the dropped read's
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), SPEED),
    ]);
    assert!(poll_once(t.read_data_by_identifier(ECU, &[Did::Speed])).is_pending());
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().sent_count, 2);
    assert!(t.transport().finished());
}

/// ``UDSSVC_ARCH_0022`` — every server answering a functional request is lent in turn,
/// named by its own address, until the window closes one response timeout after the
/// last.
#[test]
fn a_functional_read_lends_each_answer_until_the_window_closes() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), SPEED),
        Step::Ind(from(OTHER_ECU), &[0x7F, 0x22, 0x31]),
        Step::At(1_000),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    match block_on(answers.next()) {
        Some(Ok(Answer::Positive { from, mut records })) => {
            assert_eq!(from, ECU);
            assert_eq!(records.next(), Some((Did::Speed, &[0x40][..])));
        }
        other => panic!("expected the first server's answer, got {other:?}"),
    }
    assert!(matches!(
        block_on(answers.next()),
        Some(Ok(Answer::Negative {
            from: OTHER_ECU,
            code: NegativeResponseCode::RequestOutOfRange
        }))
    ));
    assert!(block_on(answers.next()).is_none());
    assert!(block_on(answers.next()).is_none());
    assert_eq!(t.transport().addressed(0), (Some(FUNCTIONAL), READ));
    assert!(t.transport().finished());
}

/// ``UDSS_LLR_0144`` — a response-pending message holds the functional window open on
/// the enhanced reload, and is not lent as an answer.
#[test]
fn a_response_pending_holds_the_functional_window_open() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), &[0x7F, 0x22, 0x78]),
        Step::At(1_000),
        Step::Ind(from(ECU), SPEED),
        Step::At(10_000),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert!(matches!(
        block_on(answers.next()),
        Some(Ok(Answer::Positive { from: ECU, .. }))
    ));
    assert!(block_on(answers.next()).is_none());
}

/// A functional request no server answers closes its window after one response timeout,
/// with no answer at all — silence is not a value.
#[test]
fn a_functional_read_nobody_answers_closes_empty() {
    let mut t = tester(&[Step::Conf(FUNCTIONAL, SResult::Ok), Step::At(1_000)]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert!(block_on(answers.next()).is_none());
    assert_eq!(t.transport().deadlines, 1);
}

/// ``UDSS_LLR_0143`` — a responder beyond the table's room is untracked, but its answer
/// is still lent.
#[test]
fn an_answer_beyond_the_responder_table_is_still_lent() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), SPEED),
        Step::Ind(from(OTHER_ECU), &[0x62, 0xF4, 0x0D, 0x41]),
        Step::At(1_000),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    let mut from = [None; 2];
    for slot in &mut from {
        if let Some(Ok(answer)) = block_on(answers.next()) {
            *slot = Some(answer.from());
        }
    }
    assert_eq!(from, [Some(ECU), Some(OTHER_ECU)]);
}

/// A refusal of a functional request surfaces at the first `next`, and ends the sequence.
#[test]
fn a_functional_read_of_nothing_is_refused_at_the_first_next() {
    let mut t = tester(&[]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[]);
    assert_eq!(
        block_on(answers.next()).map(Result::err),
        Some(Some(ClientError::Request))
    );
    assert!(block_on(answers.next()).is_none());
}

/// Cancel safety for the functional window: a `Responses` dropped half-drained leaves the
/// window open, and the next call drains it before anything else, so a late answer to
/// the same service is not taken for the physical read's.
#[test]
fn a_dropped_functional_window_is_drained_before_the_next_read() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), SPEED),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x41]), // left in the window
        Step::At(1_000),                                 // which closes
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x42]),
    ]);
    {
        let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
        assert!(matches!(block_on(answers.next()), Some(Ok(_))));
    }
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x42
    );
    assert_eq!(t.transport().sent(1).map(|s| s.after), Some(4));
}

/// The sequence the keep-alive tests share: enter the extended session at 0, its
/// response at 10, which engages physical keep-alive (``UDSS_LLR_0159``).
const ENTER: [Step; 2] = [
    Step::Conf(to(ECU), SResult::Ok),
    Step::IndAt(10, from(ECU), EXTENDED),
];

/// A tester whose transport waits up to 3 s for a response, as `tP6` lets `DoIP`.
fn patient(steps: &[Step]) -> Tester {
    Tester::new(
        Script::new(steps).with_reloads(Reloads {
            default_reload: 3_000,
            enhanced_reload: 1_000,
        }),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    )
}

/// `DiagnosticSessionControl` returns the timing the server advertised, in Table 29's
/// units, and puts the server's session under keep-alive.
#[test]
fn entering_a_session_returns_its_timing() {
    let mut t = patient(&ENTER);
    assert_eq!(
        block_on(t.diagnostic_session_control(
            ECU,
            DiagnosticSessionType::ExtendedDiagnosticSession
        ))
        .ok(),
        Some(Response::Positive(SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }))
    );
    assert_eq!(t.transport().sent_bytes(0), ENTER_EXTENDED);
}

/// ISO 14229-2:2021 9.7 — once the server is in a non-default session, `idle_until`
/// sends a `TesterPresent` every `tS3_Client`, and only when one falls due.
#[test]
fn idling_sends_a_keep_alive_every_s3_client() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::At(2_010),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(4_010),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(4_500),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(block_on(t.idle_until(Timestamp(4_500))), Ok(()));
    let s = t.transport();
    assert_eq!(s.addressed(1), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.addressed(2), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(
        (s.sent(1).map(|x| x.at), s.sent(2).map(|x| x.at)),
        (Some(2_010), Some(4_010))
    );
    assert_eq!(s.sent_count, 3);
}

/// No `TesterPresent` while a request awaits its response, even a
/// silent one outlasting `tS3_Client` (2 040 ms against 2 000): the request stops the
/// keep-alive (``UDSS_LLR_0160``) and its answer restarts it (``UDSS_LLR_0161``).
#[test]
fn no_keep_alive_while_a_silent_response_outlasts_s3_client() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(2_050, from(ECU), SPEED),
        Step::At(4_050),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(4_100),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().sent_count, 2);
    assert_eq!(block_on(t.idle_until(Timestamp(4_100))), Ok(()));
    let s = t.transport();
    assert_eq!(s.addressed(2), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(4_050));
}

/// With a response-pending cycle, still no `TesterPresent` until the final response.
#[test]
fn no_keep_alive_through_a_response_pending_cycle() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(1_500, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::IndAt(2_400, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::IndAt(3_300, from(ECU), SPEED),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().sent_count, 2);
    assert!(t.transport().finished());
}

/// A read whose repeats are spent resets the channel, and the
/// keep-alive the read stopped starts again (``UDSS_LLR_0180``), so it keeps coming.
#[test]
fn a_keep_alive_survives_a_timed_out_read() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(3_100),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(6_200),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(9_300),
        Step::At(11_300),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(11_400),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::Timeout)
    );
    assert_eq!(block_on(t.idle_until(Timestamp(11_400))), Ok(()));
    let s = t.transport();
    assert_eq!(s.addressed(4), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(4).map(|x| x.at), Some(11_300));
}

/// ISO 14229-1:2020 Table 29 — `P2*Server_max` is read in 10 ms units
/// and becomes the wait after a response-pending message. A silent 2 s, then a `0x78`
/// every 2.5 s, three times, then the answer: 2.5 s is past the transport's own 1 s
/// enhanced reload, and past the 500 ms the same value would mean read as milliseconds,
/// but inside the 5 s the server advertised.
#[test]
fn response_pending_every_two_and_a_half_seconds_survives_p2_star() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(2_010, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::IndAt(4_510, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::IndAt(7_010, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::IndAt(9_510, from(ECU), SPEED),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert_eq!(t.transport().deadlines, 0);
}

/// The other side: without the server's `P2*Server_max`, the same
/// cadence outlasts the transport's 1 s enhanced reload and times out.
#[test]
fn response_pending_every_two_and_a_half_seconds_times_out_without_p2_star() {
    let mut t = patient(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(2_000, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::At(3_001), // the 1 s enhanced window runs out before the next 0x78
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(6_100),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(9_200),
    ]);
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::Timeout)
    );
}

/// ISO 14229-2:2021 10.2.4 Figure 17 keys p and q: in functional keep-alive the
/// client-wide `tS3_Client` is not stopped by a request, so it falls due mid-exchange, and
/// the suppressed `TesterPresent` goes to the functional group on time. Nothing answers
/// it, so it cannot be taken for the exchange's answer, and holding it would let every
/// other server's session lapse during a long exchange.
#[test]
fn a_functional_keep_alive_due_mid_exchange_goes_on_time() {
    let mut t = FunctionalTester::new(
        Script::new(&[
            ENTER[0],
            ENTER[1],
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(2_000),
            Step::Conf(FUNCTIONAL, SResult::Ok),
            Step::IndAt(2_050, from(ECU), SPEED),
        ])
        .with_reloads(Reloads {
            default_reload: 3_000,
            enhanced_reload: 1_000,
        }),
        TESTER,
        KeepAlive::functional(S3_CLIENT, GROUP),
        TIMING,
    );
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let read = block_on(t.read_data_by_identifier(ECU, &[FunctionalDid::Speed]));
    assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
    let s = t.transport();
    assert_eq!(s.addressed(1), (Some(to(ECU)), READ));
    assert_eq!(s.addressed(2), (Some(FUNCTIONAL), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(2_000));
    assert!(s.finished());
}

const THIRD_ECU: Address = Address(0x0012);

/// ``UDSS_LLR_0125`` — with every channel slot taken, a read to a third server withdraws
/// an idle channel to make room, never one whose server is in a non-default session: that
/// server's keep-alive still comes when it falls due.
#[test]
fn a_full_client_withdraws_an_idle_channel_to_make_room() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), SPEED),
        Step::Conf(to(THIRD_ECU), SResult::Ok),
        Step::Ind(from(THIRD_ECU), SPEED),
        Step::At(2_010),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(2_100),
    ]);
    let entered = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(OTHER_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(THIRD_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(block_on(t.idle_until(Timestamp(2_100))), Ok(()));
    let s = t.transport();
    assert_eq!(s.addressed(3), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(3).map(|x| x.at), Some(2_010));
    assert!(s.finished());
}

/// ``UDSS_LLR_0185`` — with every slot held by a server in a non-default session, none
/// is idle, so a read to a third server is refused before anything is sent.
#[test]
fn a_full_client_with_no_idle_channel_refuses_a_new_server() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), EXTENDED),
    ]);
    for ecu in [ECU, OTHER_ECU] {
        let entered = block_on(t.diagnostic_session_control(
            ecu,
            DiagnosticSessionType::ExtendedDiagnosticSession,
        ));
        assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
    }
    assert_eq!(
        block_on(t.read_data_by_identifier(THIRD_ECU, &[Did::Speed])).err(),
        Some(ClientError::NoChannel)
    );
    assert_eq!(t.transport().sent_count, 2);
}

const GROUP_2: Address = Address(0xE401);
const FUNCTIONAL_2: Ai = Ai {
    ta: GROUP_2,
    ..FUNCTIONAL
};

/// A functional-keep-alive tester whose transport waits up to 3 s for a response.
fn functional_patient(script: Script) -> FunctionalTester {
    FunctionalTester::new(
        script.with_reloads(Reloads {
            default_reload: 3_000,
            enhanced_reload: 1_000,
        }),
        TESTER,
        KeepAlive::functional(S3_CLIENT, GROUP),
        TIMING,
    )
}

/// ``UDSS_LLR_0157`` — a functional keep-alive confirmed failed leaves the client-wide
/// timer stopped, and its repeat is the client's: it is sent again once the spacing the
/// failure started has run out, and its success restarts the timer.
#[test]
fn a_failed_functional_keep_alive_is_repeated() {
    let mut t = functional_patient(Script::new(&[
        ENTER[0],
        ENTER[1],
        Step::At(2_000),
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(2_010),
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::At(4_010),
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::At(4_100),
    ]));
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(block_on(t.idle_until(Timestamp(4_100))), Ok(()));
    let s = t.transport();
    let at = |i| s.sent(i).map(|x| (x.ai, x.at));
    assert_eq!(
        [at(1), at(2), at(3)],
        [
            Some((FUNCTIONAL, 2_000)),
            Some((FUNCTIONAL, 2_010)),
            Some((FUNCTIONAL, 4_010))
        ]
    );
    assert!(s.finished());
}

/// ``UDSS_LLR_0060`` — a request the transport refuses is confirmed failed to the session
/// layer by the client, so the refusal ends that exchange and the next request to the
/// server goes out.
#[test]
fn a_request_the_transport_refuses_does_not_block_the_next() {
    let mut t = Tester::new(
        Script::new(&[
            Step::At(10), // the spacing its failed confirmation started
            Step::Conf(to(ECU), SResult::Ok),
            Step::Ind(from(ECU), SPEED),
        ])
        .refusing(0),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    );
    assert_eq!(
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed])).err(),
        Some(ClientError::Transport(()))
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    assert!(t.transport().finished());
}

/// ``UDSSVC_ARCH_0022``, ``UDSS_LLR_0148`` — an answer delivered with the clock already
/// past the window is lent, and the window's expiry, seen in the same event, still closes
/// the sequence.
#[test]
fn an_answer_arriving_as_the_window_closes_ends_the_sequence() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::IndAt(100, from(ECU), SPEED),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert!(matches!(
        block_on(answers.next()),
        Some(Ok(Answer::Positive { from: ECU, .. }))
    ));
    assert!(block_on(answers.next()).is_none());
}

/// ``UDSS_LLR_0125`` — a dropped window is drained before the next functional read opens
/// its channel, so the channel it is draining on is not the one withdrawn to make room.
#[test]
fn a_functional_read_to_another_group_waits_for_the_dropped_window() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), SPEED),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x41]),
        Step::At(1_000),
        Step::Conf(FUNCTIONAL_2, SResult::Ok),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x42]),
        Step::At(2_000),
    ]);
    {
        let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
        assert!(matches!(block_on(answers.next()), Some(Ok(_))));
    }
    let mut answers = t.read_data_by_identifier_functional(GROUP_2, &[Did::Speed]);
    match block_on(answers.next()) {
        Some(Ok(Answer::Positive { mut records, .. })) => {
            assert_eq!(records.next(), Some((Did::Speed, &[0x42][..])));
        }
        other => panic!("expected the second window's answer, got {other:?}"),
    }
    assert!(block_on(answers.next()).is_none());
    assert_eq!(t.transport().addressed(1), (Some(FUNCTIONAL_2), READ));
}

/// No keep-alive goes to the server a request awaits, but another server's falls due on
/// its own channel and goes out on time, mid-exchange.
#[test]
fn another_servers_keep_alive_is_not_held_by_an_exchange() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::At(2_010),
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(2_600, from(OTHER_ECU), SPEED),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(OTHER_ECU, &[Did::Speed])
        )),
        0x40
    );
    let s = t.transport();
    assert_eq!(s.addressed(2), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(2_010));
    assert!(s.finished());
}

/// ``UDSSVC_ARCH_0020`` — an answer already received is returned even
/// where the keep-alive held behind it then fails to go out.
#[test]
fn a_held_keep_alive_the_transport_refuses_does_not_lose_the_answer() {
    let mut t = functional_patient(
        Script::new(&[
            ENTER[0],
            ENTER[1],
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(2_000),
            Step::IndAt(2_050, from(ECU), SPEED),
        ])
        .refusing(2),
    );
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let read = block_on(t.read_data_by_identifier(ECU, &[FunctionalDid::Speed]));
    assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
}

/// A positive session change the client cannot read still put the server in its session,
/// as the session layer classified it, so its channel is not withdrawn to make room.
#[test]
fn an_unreadable_session_change_still_keeps_its_channel() {
    let mut t = patient(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(10, from(ECU), &[0x50, 0x03]),
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), SPEED),
        Step::Conf(to(THIRD_ECU), SResult::Ok),
        Step::Ind(from(THIRD_ECU), SPEED),
        Step::At(2_010),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(2_100),
    ]);
    let entered = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        entered.ok(),
        Some(Response::Malformed(MalformedResponse::Short))
    );
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(OTHER_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(THIRD_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(block_on(t.idle_until(Timestamp(2_100))), Ok(()));
    assert_eq!(t.transport().addressed(3), (Some(to(ECU)), KEEP_ALIVE));
}

/// ``UDSSVC_ARCH_0022`` — a functional window awaits every server, so one server's
/// connection closing ends nothing: the others' answers are still lent, and the window
/// closes at its own timeout, as it does for a server that stays silent.
#[test]
fn a_close_during_a_functional_window_does_not_end_it() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Close(ECU),
        Step::Ind(from(OTHER_ECU), SPEED),
        Step::At(100),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    let first = block_on(answers.next());
    assert!(
        matches!(first, Some(Ok(Answer::Positive { from, .. })) if from == OTHER_ECU),
        "{first:?}"
    );
    assert!(block_on(answers.next()).is_none());
    drop(answers);
    assert!(t.transport().finished());
}

/// ISO 14229-2:2021 Table 9 — a functional request whose transmission fails is repeated,
/// twice, and then the sequence ends with the failure.
#[test]
fn a_functional_request_that_fails_to_go_is_repeated_then_not_sent() {
    let mut t = tester(&[
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(10),
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(20),
        Step::Conf(FUNCTIONAL, FAILED),
    ]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert_eq!(
        block_on(answers.next()).map(Result::err),
        Some(Some(ClientError::NotSent))
    );
    assert!(block_on(answers.next()).is_none());
    assert_eq!(t.transport().sent_count, 3);
}

/// ``UDSSVC_ARCH_0022`` — a transport failure ends a functional sequence: the call after
/// it returns `None`.
#[test]
fn a_transport_failure_ends_a_functional_sequence() {
    let mut t = tester(&[Step::Conf(FUNCTIONAL, SResult::Ok)]);
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert_eq!(
        block_on(answers.next()).map(Result::err),
        Some(Some(ClientError::Transport(())))
    );
    assert!(block_on(answers.next()).is_none());
}

/// ``UDSS_LLR_0019`` — repeats and their response windows are timed across the wrap of
/// the 32-bit clock.
#[test]
fn repeats_are_timed_across_the_clock_wrap() {
    let start = u32::MAX - 60;
    let mut t = Tester::new(
        Script::new(&[
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(start.wrapping_add(50)), // inside the window
            Step::At(start.wrapping_add(51)), // its end, past the wrap
            Step::Conf(to(ECU), SResult::Ok),
            Step::Ind(from(ECU), SPEED),
        ])
        .starting_at(start),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
    let s = t.transport();
    assert_eq!(s.deadlines, 1);
    assert_eq!(s.sent(1).map(|x| x.at), Some(start.wrapping_add(51)));
}

/// ISO 14229-2:2021 Table 4 — after a response-pending message the client waits
/// `P2*Server_max` plus the network delay (`tP6*_Client`), so a server answering at the
/// edge of its own budget is not timed out by the time the answer takes to arrive.
#[test]
fn the_wait_after_a_response_pending_includes_the_network_delay() {
    let steps = [
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(2_010, from(ECU), &[0x7F, 0x22, 0x78]),
        Step::At(7_015), // past P2* alone, inside P2* plus the delay
        Step::IndAt(7_060, from(ECU), SPEED), // 5 050 ms later: P2* plus 50 ms in transit
    ];
    let read = |network_delay| {
        let mut t = Tester::new(
            Script::new(&steps).with_reloads(Reloads {
                default_reload: 3_000,
                enhanced_reload: 1_000,
            }),
            TESTER,
            KeepAlive::physical(S3_CLIENT),
            ClientTiming::new(10, 10, network_delay),
        );
        let _ = block_on(t.diagnostic_session_control(
            ECU,
            DiagnosticSessionType::ExtendedDiagnosticSession,
        ));
        block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))
            .map(|r| matches!(r, Response::Positive(_)))
    };
    assert_eq!(read(100), Ok(true));
    assert_ne!(read(0), Ok(true));
}

const DEFAULT_SESSION: &[u8] = &[0x50, 0x01, 0x00, 0x32, 0x01, 0xF4];

/// ``UDSS_LLR_0148`` — a response window that expires while the client is sending other
/// servers' keep-alives is still the exchange's timeout, and ISO 14229-2:2021 Table 9's
/// repeat follows. Here the first keep-alive takes the clock past the window, and the
/// second's input is the one that finds it expired.
#[test]
fn a_timeout_found_while_sending_keep_alives_is_still_repeated() {
    let mut t = WideTester::new(
        Script::new(&[
            Step::Conf(to(ECU), SResult::Ok),
            Step::IndAt(110, from(ECU), EXTENDED),
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::IndAt(220, from(OTHER_ECU), EXTENDED),
            Step::Slip(2_230),
            Step::Conf(to(THIRD_ECU), SResult::Ok), // window to 2 281; both keep-alives due
            Step::Conf(to(ECU), SResult::Ok),
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::Conf(to(THIRD_ECU), SResult::Ok),
            Step::Ind(from(THIRD_ECU), SPEED),
        ])
        .costing(100),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    );
    for ecu in [ECU, OTHER_ECU] {
        let entered = block_on(t.diagnostic_session_control(
            ecu,
            DiagnosticSessionType::ExtendedDiagnosticSession,
        ));
        assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
    }
    let read = block_on(t.read_data_by_identifier(THIRD_ECU, &[WideDid::Speed]));
    assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
    let s = t.transport();
    assert_eq!(s.addressed(3), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.addressed(4), (Some(to(OTHER_ECU)), KEEP_ALIVE));
    assert_eq!(s.addressed(5), (Some(to(THIRD_ECU)), READ));
}

/// A server answering a functional request is awaited too, so its
/// physical keep-alive is held until the window closes.
#[test]
fn no_physical_keep_alive_while_a_functional_window_is_open() {
    let mut t = patient(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::At(2_010),
        Step::IndAt(2_500, from(ECU), SPEED),
        Step::At(6_000),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[Did::Speed]);
    assert!(matches!(block_on(answers.next()), Some(Ok(_))));
    assert!(block_on(answers.next()).is_none());
    let s = t.transport();
    assert_eq!(s.addressed(2), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(6_000));
}

/// ``UDSS_LLR_0184`` — in functional keep-alive, once no server this client put in a
/// non-default session remains in one, the client-wide keep-alive is released: a
/// physically addressed return to the default session does not end it on its own
/// (``UDSS_LLR_0158`` reads only a functional one).
#[test]
fn functional_keep_alive_ends_when_the_last_server_leaves_its_session() {
    let mut t = functional_patient(Script::new(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(20, from(ECU), DEFAULT_SESSION),
        Step::At(4_500),
    ]));
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let left =
        block_on(t.diagnostic_session_control(ECU, DiagnosticSessionType::DefaultSession));
    assert!(matches!(left, Ok(Response::Positive(_))), "{left:?}");
    assert_eq!(block_on(t.idle_until(Timestamp(4_500))), Ok(()));
    assert_eq!(t.transport().sent_count, 2);
}

/// ``UDSS_LLR_0136`` — the session layer opens the response window at the confirmation,
/// so a response before it answers nothing; the one after it is the answer.
#[test]
fn a_response_before_the_confirmation_is_not_the_answer() {
    let mut t = tester(&[
        Step::Ind(from(ECU), SPEED),
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x62, 0xF4, 0x0D, 0x41]),
    ]);
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x41
    );
}

/// ``UDSS_LLR_0159``, ``UDSS_LLR_0125`` — a positive session change too long for the
/// buffer still put the server in its session, so its channel is kept as one in session.
#[test]
fn an_overlong_session_change_still_keeps_its_channel() {
    let mut t = patient(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::TooLong(
            from(ECU),
            &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4, 0xAA, 0xBB, 0xCC],
        ),
        Step::Conf(to(OTHER_ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), SPEED),
        Step::Conf(to(THIRD_ECU), SResult::Ok),
        Step::Ind(from(THIRD_ECU), SPEED),
        Step::At(2_000),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(2_100),
    ]);
    let entered = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert!(matches!(
        entered,
        Ok(Response::Malformed(MalformedResponse::Overlong { .. }))
    ));
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(OTHER_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(
        speed(block_on(
            t.read_data_by_identifier(THIRD_ECU, &[Did::Speed])
        )),
        0x40
    );
    assert_eq!(block_on(t.idle_until(Timestamp(2_100))), Ok(()));
    assert_eq!(t.transport().addressed(3), (Some(to(ECU)), KEEP_ALIVE));
}

/// ISO 14229-2:2021 Table 9 — a running exchange's response timeout found while a
/// keep-alive is being sent survives the transport refusing that keep-alive, so the read is
/// still repeated and then times out, rather than waiting for ever.
#[test]
fn a_timeout_found_while_a_keep_alive_is_refused_is_not_lost() {
    let mut t = WideTester::new(
        Script::new(&[
            Step::Conf(to(ECU), SResult::Ok),
            Step::IndAt(60, from(ECU), EXTENDED),
            Step::Conf(to(THIRD_ECU), SResult::Ok),
            Step::IndAt(110, from(THIRD_ECU), EXTENDED),
            Step::At(2_000),
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::At(2_115),
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::At(2_400),
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::At(2_600),
        ])
        .with_reloads(Reloads {
            default_reload: 75,
            enhanced_reload: 1_000,
        })
        .costing(50)
        .refusing(4),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    );
    let extended = DiagnosticSessionType::ExtendedDiagnosticSession;
    let _ = block_on(t.diagnostic_session_control(ECU, extended));
    let _ = block_on(t.diagnostic_session_control(THIRD_ECU, extended));
    assert_eq!(block_on(t.idle_until(Timestamp(2_000))), Ok(()));
    let read = block_on(t.read_data_by_identifier(OTHER_ECU, &[WideDid::Speed]));
    assert_eq!(read.map(|_| ()), Err(ClientError::Timeout));
}

/// ``UDSS_LLR_0157`` — a functional keep-alive still failing after
/// ISO 14229-2:2021 Table 9's two repeats goes again one `tS3_Client` after the last
/// failure, as a physical one does, rather than ending the keep-alive for good.
#[test]
fn a_functional_keep_alive_given_up_goes_again_a_period_later() {
    let mut t = functional_patient(Script::new(&[
        ENTER[0],
        ENTER[1],
        Step::At(2_000),
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(2_010),
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(2_020),
        Step::Conf(FUNCTIONAL, FAILED),
        Step::At(4_020),
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::At(4_100),
    ]));
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(block_on(t.idle_until(Timestamp(4_100))), Ok(()));
    let s = t.transport();
    let at = |i| s.sent(i).map(|x| (x.ai, x.at));
    assert_eq!(at(4), Some((FUNCTIONAL, 4_020)));
    assert!(s.finished());
}

/// ``UDSSVC_ARCH_0022`` — no keep-alive goes while a functional window is open: it awaits
/// every server, and the functional keep-alive would share its channel. It goes once the
/// window closes, when its timeout is exceeded (``UDSS_LLR_0148``).
#[test]
fn a_functional_keep_alive_waits_for_a_functional_window() {
    let mut t = functional_patient(Script::new(&[
        ENTER[0],
        ENTER[1],
        Step::Conf(FUNCTIONAL, SResult::Ok),
        Step::Ind(from(ECU), SPEED),
        Step::At(2_000),
        Step::At(3_011),
        Step::Conf(FUNCTIONAL, SResult::Ok),
    ]));
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let mut answers = t.read_data_by_identifier_functional(GROUP, &[FunctionalDid::Speed]);
    assert!(matches!(block_on(answers.next()), Some(Ok(_))));
    assert!(block_on(answers.next()).is_none());
    drop(answers);
    let s = t.transport();
    assert_eq!(s.addressed(2), (Some(FUNCTIONAL), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(3_011));
}

/// ``UDSS_LLR_0180``, ``UDSS_LLR_0182`` — a read dropped while the transport is taking
/// its request is reset like any sent one, so the abandoned request's confirmation
/// restarts the server's keep-alive, which the request had stopped (``UDSS_LLR_0160``).
#[test]
fn a_read_dropped_inside_the_transport_keeps_the_server_alive() {
    let mut t = Tester::new(
        Script::new(&[
            ENTER[0],
            ENTER[1],
            Step::Conf(to(ECU), SResult::Ok), // the dropped read's
            Step::At(2_010),
            Step::At(2_100),
        ])
        .stalling(1),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        TIMING,
    );
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert!(poll_once(t.read_data_by_identifier(ECU, &[Did::Speed])).is_pending());
    assert_eq!(block_on(t.idle_until(Timestamp(2_100))), Ok(()));
    let s = t.transport();
    assert_eq!(s.addressed(2), (Some(to(ECU)), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(2_010));
}

/// ISO 14229-1:2020 10.2 Table 29 — a positive `DiagnosticSessionControl` response echoes
/// the session requested; one echoing another is a late reply to an earlier change, not
/// the answer, and neither its session nor its timing is taken.
#[test]
fn a_session_change_reply_for_another_session_is_not_the_answer() {
    const PROGRAMMING: &[u8] = &[0x50, 0x02, 0x00, 0x19, 0x00, 0x64];
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), PROGRAMMING),
        Step::Ind(from(ECU), EXTENDED),
    ]);
    let entered = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        entered,
        Ok(Response::Positive(SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }))
    );
    assert!(t.transport().finished());
}

/// ISO 14229-1:2020 Table 188 — a positive `ReadDataByIdentifier` response carries at
/// least one identifier and its record, so one with none is too short.
#[test]
fn a_positive_read_with_no_records_is_short() {
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), &[0x62]),
    ]);
    let read = block_on(t.read_data_by_identifier(ECU, &[Did::Speed]));
    assert!(
        matches!(read, Ok(Response::Malformed(MalformedResponse::Short))),
        "{read:?}"
    );
}

/// ``UDSS_LLR_0184`` — in functional keep-alive, a session change that never completes
/// leaves no server this client knows to be in session, so the functional keep-alive its
/// confirmation started (``UDSS_LLR_0155``) is released.
#[test]
fn a_session_change_that_times_out_releases_functional_keep_alive() {
    let mut t = FunctionalTester::new(
        Script::new(&[
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(60),
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(120),
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(180),
            Step::At(10_000),
        ]),
        TESTER,
        KeepAlive::functional(S3_CLIENT, GROUP),
        TIMING,
    );
    let entered = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(entered, Err(ClientError::Timeout));
    assert_eq!(block_on(t.idle_until(Timestamp(10_000))), Ok(()));
    assert_eq!(t.transport().sent_count, 3);
}

/// ``UDSS_LLR_0184`` — so does a close taking the last server in session out of it.
#[test]
fn a_close_of_the_last_server_in_session_releases_functional_keep_alive() {
    let mut t = functional_patient(Script::new(&[
        ENTER[0],
        ENTER[1],
        Step::Close(ECU),
        Step::At(10_000),
    ]));
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(block_on(t.idle_until(Timestamp(10_000))), Ok(()));
    assert_eq!(t.transport().sent_count, 1);
}

/// ISO 14229-2:2021 9.2 Table 4 — a server's `P2Server_max`, plus the network delay,
/// becomes the wait for its response (`tP6_Client`) where that is longer than the
/// transport's, so a server answering within the time it advertised is not timed out.
#[test]
fn a_session_change_sets_the_response_wait_from_p2() {
    const SLOW: &[u8] = &[0x50, 0x03, 0x00, 0xC8, 0x01, 0xF4];
    let mut t = tester(&[
        Step::Conf(to(ECU), SResult::Ok),
        Step::IndAt(10, from(ECU), SLOW),
        Step::Conf(to(ECU), SResult::Ok),
        Step::At(150),
        Step::Ind(from(ECU), SPEED),
    ]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
}

/// ``UDSS_LLR_0155`` — a close taking the last server in session out of it while a
/// session change to another is running leaves the functional keep-alive that change's
/// confirmation engaged, so the server it puts in session is kept alive.
#[test]
fn a_close_during_a_session_change_keeps_functional_keep_alive() {
    let mut t = WideFunctionalTester::new(
        Script::new(&[
            ENTER[0],
            ENTER[1],
            Step::Conf(to(OTHER_ECU), SResult::Ok),
            Step::Close(ECU),
            Step::IndAt(20, from(OTHER_ECU), EXTENDED),
            Step::At(2_000),
            Step::Conf(FUNCTIONAL, SResult::Ok),
            Step::At(2_100),
        ]),
        TESTER,
        KeepAlive::functional(S3_CLIENT, GROUP),
        TIMING,
    );
    let extended = DiagnosticSessionType::ExtendedDiagnosticSession;
    let _ = block_on(t.diagnostic_session_control(ECU, extended));
    let entered = block_on(t.diagnostic_session_control(OTHER_ECU, extended));
    assert!(matches!(entered, Ok(Response::Positive(_))), "{entered:?}");
    assert_eq!(block_on(t.idle_until(Timestamp(2_100))), Ok(()));
    assert_eq!(t.transport().addressed(2), (Some(FUNCTIONAL), KEEP_ALIVE));
}

/// Ending a session closes the transport, once.
#[test]
fn close_closes_the_transport_once() {
    let mut t = tester(&[]);
    assert_eq!(block_on(t.close()), Ok(()));
    assert_eq!(t.transport().closes, 1);
}

/// A request a dropped call left unconfirmed is confirmed failed before `close`
/// returns, so nothing about it is left for the next call to read.
#[test]
fn close_reads_the_confirm_a_dropped_call_left_owed() {
    let mut t = tester(&[
        Step::Pend,
        Step::Conf(to(ECU), FAILED),
        Step::At(100),
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(ECU), SPEED),
    ]);
    assert!(poll_once(t.read_data_by_identifier(ECU, &[Did::Speed])).is_pending());

    assert_eq!(block_on(t.close()), Ok(()));
    assert_eq!(t.transport().cursor, 2);
    assert_eq!(
        speed(block_on(t.read_data_by_identifier(ECU, &[Did::Speed]))),
        0x40
    );
}

/// ``UDSS_LLR_0184`` — a client that ends its session keeps nothing alive: no
/// `TesterPresent` follows the close.
#[test]
fn close_ends_the_keep_alive() {
    let mut t = patient(&[ENTER[0], ENTER[1], Step::At(4_500)]);
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );

    assert_eq!(block_on(t.close()), Ok(()));
    assert_eq!(block_on(t.idle_until(Timestamp(4_500))), Ok(()));
    assert_eq!(t.transport().sent_count, 1);
}
