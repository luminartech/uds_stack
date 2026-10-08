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
    Address, Ai, Answer, ClientError, DataIdentifier, DiagnosticSessionType,
    FunctionalKeepAlive, KeepAlive, Mtype, NegativeResponseCode, PhysicalKeepAlive,
    RecordError, Reloads, Response, SResult, SessionTiming, Spacing, TaType, Timestamp,
    uds_client,
};
use uds_session::TransportError;

const TESTER: Address = Address(0x0E80);
const ECU: Address = Address(0x0010);
const OTHER_ECU: Address = Address(0x0011);
const GROUP: Address = Address(0xE400);
const S3_CLIENT: u32 = 2_000;
const SPACING: Spacing = Spacing {
    physical: 10,
    functional: 10,
};

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

fn tester(steps: &[Step]) -> Tester {
    Tester::new(
        Script::new(steps),
        TESTER,
        KeepAlive::physical(S3_CLIENT),
        SPACING,
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

/// Table 9 and ``UDSS_LLR_0171`` — a failed transmission is repeated too, each repeat
/// held until the spacing timer the failure started has run out, not reported.
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
        Some(Response::Malformed(RecordError::Overlong {
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
        Step::Conf(to(ECU), SResult::Ok),
        Step::Ind(from(OTHER_ECU), &[0x62, 0xF4, 0x0D, 0x41]),
        Step::Ind(from(ECU), SPEED),
    ]);
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

/// The sequence the #17 keep-alive tests share: enter the extended session at 0, its
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
        SPACING,
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

/// Issue #17 item 1 — no `TesterPresent` while a request awaits its response, even a
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

/// Issue #17 item 1, with a response-pending cycle: still no `TesterPresent` until the
/// final response.
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

/// Issue #30 end to end: a read whose repeats are spent resets the channel, and the
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

/// Issue #17 item 4, ISO 14229-1:2020 Table 29 — `P2*Server_max` is read in 10 ms units
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

/// Issue #17 item 4, the other side: without the server's `P2*Server_max`, the same
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

/// Issue #17 item 1 in functional keep-alive: the client-wide `tS3_Client` is not
/// stopped by a request, so it falls due mid-exchange; the `TesterPresent` is held until
/// the answer, then sent to the functional group.
#[test]
fn a_functional_keep_alive_due_mid_exchange_waits_for_the_answer() {
    let mut t = FunctionalTester::new(
        Script::new(&[
            ENTER[0],
            ENTER[1],
            Step::Conf(to(ECU), SResult::Ok),
            Step::At(2_000),
            Step::IndAt(2_050, from(ECU), SPEED),
        ])
        .with_reloads(Reloads {
            default_reload: 3_000,
            enhanced_reload: 1_000,
        }),
        TESTER,
        KeepAlive::functional(S3_CLIENT, GROUP),
        SPACING,
    );
    let _ = block_on(
        t.diagnostic_session_control(ECU, DiagnosticSessionType::ExtendedDiagnosticSession),
    );
    let read = block_on(t.read_data_by_identifier(ECU, &[FunctionalDid::Speed]));
    assert!(matches!(read, Ok(Response::Positive(_))), "{read:?}");
    let s = t.transport();
    assert_eq!(s.addressed(1), (Some(to(ECU)), READ));
    assert_eq!(s.addressed(2), (Some(FUNCTIONAL), KEEP_ALIVE));
    assert_eq!(s.sent(2).map(|x| x.at), Some(2_050));
    assert!(s.finished());
}
