//! `DoIpClientTransport` over a scripted [`TesterConnection`]: the mapping from
//! `DoIP_Data` onto the transport seam (ISO 14229-5:2022 REQ 4.3 Table 4, REQ 4.4
//! Table 5), and what the transport absorbs of the tester's own constraints.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test: a failed expectation is the test failing"
)]

use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, Refusal, TesterAddress,
    TesterConnection, Timestamp,
};
use simple_doip::{LogicalAddress, TaType};
use uds_on_ip::DoIpClientTransport;
use uds_services::{AfterSend, ClientTransport, TransportEvent, UdsTransport};
use uds_session::{Address, Ai, Mtype, Reloads, SResult, TransportError};

const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const ENTITY: LogicalAddress = LogicalAddress(0x0001);
const OTHER_TESTER: LogicalAddress = LogicalAddress(0x0E01);
const MAX_PDU: usize = 64;
const RELOADS: Reloads = Reloads {
    default_reload: 50,
    enhanced_reload: 5_000,
};

/// What the scripted entity does next.
#[derive(Debug, Clone)]
enum Entity {
    /// Acknowledges the outstanding request with this outcome.
    Acks(DoIpResult),
    /// Sends a diagnostic message.
    Sends {
        sa: LogicalAddress,
        ta: LogicalAddress,
        pdu: Vec<u8>,
    },
    /// Sends a diagnostic message longer than the caller's buffer.
    SendsLong {
        ta: LogicalAddress,
        pdu: Vec<u8>,
        length: usize,
    },
    /// Sends a message of a payload type the tester does not model.
    SendsUnmodelled(u16, Vec<u8>),
    /// Sends the same, longer than the caller's buffer.
    SendsUnmodelledLong(u16, Vec<u8>, usize),
    /// The connection ends.
    Closes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Request(LogicalAddress, TaType, Vec<u8>),
    Reconnect,
    Close,
}

/// A [`TesterConnection`] playing a script, keeping the tester's contract: one request
/// at a time, every accepted request confirmed, `Closed` once per end and then quiet
/// until the deadline.
#[derive(Debug)]
struct Scripted {
    script: VecDeque<Entity>,
    connected: bool,
    closed_reported: bool,
    outstanding: Option<(LogicalAddress, TaType)>,
    owed: VecDeque<ConnectionEvent<'static>>,
    calls: Vec<Call>,
    failing_reconnects: usize,
    now: u32,
    /// How many times `next_event` was called while closed, its `Closed` reported.
    quiet_calls: usize,
}

impl Scripted {
    fn new(script: impl IntoIterator<Item = Entity>) -> Self {
        Self {
            script: script.into_iter().collect(),
            connected: true,
            closed_reported: false,
            outstanding: None,
            owed: VecDeque::new(),
            calls: Vec::new(),
            failing_reconnects: 0,
            now: 0,
            quiet_calls: 0,
        }
    }

    fn give_up(&mut self, result: DoIpResult) {
        self.connected = false;
        if let Some((ta, ta_type)) = self.outstanding.take() {
            self.owed.push_back(ConnectionEvent::Confirm {
                sa: TESTER,
                ta,
                ta_type,
                result,
            });
        }
    }

    fn requests(&self) -> Vec<Vec<u8>> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                Call::Request(_, _, pdu) => Some(pdu.clone()),
                Call::Reconnect | Call::Close => None,
            })
            .collect()
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "a scripted connection answers at once, as the trait's async signature allows"
)]
impl DiagnosticConnection for Scripted {
    type Error = core::convert::Infallible;
    const MAX_PDU: usize = MAX_PDU;

    async fn request(
        &mut self,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Refusal> {
        if !self.connected {
            return Err(Refusal::NotConnected);
        }
        if self.outstanding.is_some() || !self.owed.is_empty() {
            return Err(Refusal::NoRoom);
        }
        if pdu.is_empty() {
            return Err(Refusal::EmptyPdu);
        }
        if pdu.len() > MAX_PDU {
            return Err(Refusal::PduTooLarge {
                len: pdu.len(),
                max: MAX_PDU,
            });
        }
        self.outstanding = Some((ta, ta_type));
        self.calls.push(Call::Request(ta, ta_type, pdu.to_vec()));
        Ok(())
    }

    fn now(&self) -> Timestamp {
        Timestamp(self.now)
    }

    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<ConnectionEvent<'b>, Self::Error> {
        if let Some(owed) = self.owed.pop_front() {
            return Ok(owed);
        }
        if !self.connected {
            if !self.closed_reported {
                self.closed_reported = true;
                return Ok(ConnectionEvent::Closed);
            }
            self.quiet_calls = self.quiet_calls.saturating_add(1);
            if let Some(deadline) = deadline {
                self.now = deadline.0;
            }
            return Ok(ConnectionEvent::Deadline);
        }
        let Some(step) = self.script.pop_front() else {
            if let Some(deadline) = deadline {
                self.now = deadline.0;
            }
            return Ok(ConnectionEvent::Deadline);
        };
        Ok(match step {
            Entity::Acks(result) => {
                let (ta, ta_type) = self.outstanding.take().expect("a request to ack");
                ConnectionEvent::Confirm {
                    sa: TESTER,
                    ta,
                    ta_type,
                    result,
                }
            }
            Entity::Sends { sa, ta, pdu } => {
                let pdu = copy(buf, &pdu);
                ConnectionEvent::Indication {
                    sa,
                    ta,
                    ta_type: ta.default_ta_type(),
                    pdu,
                }
            }
            Entity::SendsLong { ta, pdu, length } => {
                let pdu = copy(buf, &pdu);
                ConnectionEvent::IndicationTruncated {
                    sa: ENTITY,
                    ta,
                    ta_type: ta.default_ta_type(),
                    pdu,
                    length,
                }
            }
            Entity::SendsUnmodelled(payload_type, data) => ConnectionEvent::Unmodelled {
                payload_type,
                data: copy(buf, &data),
            },
            Entity::SendsUnmodelledLong(payload_type, data, length) => {
                ConnectionEvent::UnmodelledTruncated {
                    payload_type,
                    data: copy(buf, &data),
                    length,
                }
            }
            Entity::Closes => {
                self.give_up(DoIpResult::NoSocket);
                self.owed.pop_front().unwrap_or_else(|| {
                    self.closed_reported = true;
                    ConnectionEvent::Closed
                })
            }
        })
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "a scripted connection answers at once, as the trait's async signature allows"
)]
impl TesterConnection for Scripted {
    type ReconnectError = &'static str;
    type CloseError = core::convert::Infallible;
    type IoError = core::convert::Infallible;

    fn address(&self) -> TesterAddress {
        TesterAddress::new(TESTER).unwrap()
    }

    fn io_error(&self) -> Option<&Self::IoError> {
        None
    }

    async fn reconnect(&mut self) -> Result<(), Self::ReconnectError> {
        self.calls.push(Call::Reconnect);
        self.give_up(DoIpResult::NoSocket);
        if self.failing_reconnects > 0 {
            self.failing_reconnects = self.failing_reconnects.saturating_sub(1);
            return Err("refused");
        }
        self.connected = true;
        self.closed_reported = false;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), Self::CloseError> {
        self.calls.push(Call::Close);
        self.give_up(DoIpResult::NoSocket);
        Ok(())
    }
}

fn copy<'b>(buf: &'b mut [u8], bytes: &[u8]) -> &'b [u8] {
    let n = buf.len().min(bytes.len());
    let (head, _) = buf.split_at_mut(n);
    head.copy_from_slice(&bytes[..n]);
    head
}

/// Runs a future that never waits on anything outside the test.
fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..1_000 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
    panic!("the future did not complete")
}

type Transport = DoIpClientTransport<Scripted, 256>;

fn transport(script: impl IntoIterator<Item = Entity>) -> Transport {
    DoIpClientTransport::new(Scripted::new(script), RELOADS)
}

const fn ai(ta: LogicalAddress, ta_type: uds_session::TaType) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: Address(TESTER.0),
        ta: Address(ta.0),
        ta_type,
    }
}

const TO_ENTITY: Ai = ai(ENTITY, uds_session::TaType::Physical);
const FROM_ENTITY: Ai = Ai {
    mtype: Mtype::Diag,
    sa: Address(ENTITY.0),
    ta: Address(TESTER.0),
    ta_type: uds_session::TaType::Physical,
};
const READ: &[u8] = &[0x22, 0xF1, 0x90];
const ANSWER: &[u8] = &[0x62, 0xF1, 0x90, 0x41];
const KEEP_ALIVE: &[u8] = &[0x3E, 0x80];

/// `DoIP_Result`'s position in ISO 13400-2:2019 8.2.5's order, as `s_result` maps it.
const fn failed(result: DoIpResult) -> SResult {
    let position = match result {
        DoIpResult::Ok => return SResult::Ok,
        DoIpResult::HdrError => 1,
        DoIpResult::TimeoutA => 2,
        DoIpResult::UnknownSa => 3,
        DoIpResult::InvalidSa => 4,
        DoIpResult::UnknownTa => 5,
        DoIpResult::MessageTooLarge => 6,
        DoIpResult::OutOfMemory => 7,
        DoIpResult::TargetUnreachable => 8,
        DoIpResult::NoLink => 9,
        DoIpResult::NoSocket => 10,
        DoIpResult::Error => 11,
    };
    SResult::Transport(TransportError(position))
}

fn send(t: &mut Transport, ai: Ai, pdu: &[u8]) {
    run(t.t_data_req(ai, pdu, AfterSend::Continue)).unwrap();
}

/// The next event, with its data copied out of the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Ind(Ai, Vec<u8>),
    TooLong(Ai, Vec<u8>, Option<usize>),
    Conf(Ai, SResult),
    Periodic(Ai, u8, Vec<u8>),
    Closed(Address, bool),
    Deadline,
}

fn next(t: &mut Transport) -> Seen {
    next_until(t, None)
}

fn next_until(t: &mut Transport, deadline: Option<uds_session::Timestamp>) -> Seen {
    let mut buf = [0; 32];
    match run(t.next_event(&mut buf, deadline)).unwrap() {
        TransportEvent::DataInd { ai, data } => Seen::Ind(ai, data.to_vec()),
        TransportEvent::DataTooLong { ai, data, declared } => {
            Seen::TooLong(ai, data.to_vec(), declared)
        }
        TransportEvent::DataConf { ai, result } => Seen::Conf(ai, result),
        TransportEvent::Periodic { ai, pdid, data } => {
            Seen::Periodic(ai, pdid, data.to_vec())
        }
        TransportEvent::Closed { peer, expected } => Seen::Closed(peer, expected),
        TransportEvent::Deadline => Seen::Deadline,
    }
}

fn answer(pdu: &[u8]) -> Entity {
    Entity::Sends {
        sa: ENTITY,
        ta: TESTER,
        pdu: pdu.to_vec(),
    }
}

// --- the mapping ------------------------------------------------------------------------

/// ISO 14229-5:2022 REQ 4.3 Table 4: `DoIP_Data.indication` is `T_Data.ind`, with
/// REQ 4.4 Table 5's addressing.
#[test]
fn a_message_to_the_tester_is_a_data_ind() {
    let mut t = transport([answer(ANSWER)]);
    assert_eq!(next(&mut t), Seen::Ind(FROM_ENTITY, ANSWER.to_vec()));
}

/// A message to another address than this tester's is not this tester's response.
#[test]
fn a_message_to_another_address_is_dropped() {
    let mut t = transport([
        Entity::Sends {
            sa: ENTITY,
            ta: OTHER_TESTER,
            pdu: ANSWER.to_vec(),
        },
        answer(ANSWER),
    ]);
    assert_eq!(next(&mut t), Seen::Ind(FROM_ENTITY, ANSWER.to_vec()));
}

/// A message longer than the buffer lent is `DataTooLong`, with the length the generic
/// header declared.
#[test]
fn a_long_message_is_data_too_long_with_its_length() {
    let mut t = transport([Entity::SendsLong {
        ta: TESTER,
        pdu: ANSWER.to_vec(),
        length: 900,
    }]);
    assert_eq!(
        next(&mut t),
        Seen::TooLong(FROM_ENTITY, ANSWER.to_vec(), Some(900))
    );
}

/// REQ 4.3 Table 4: `DoIP_Data.confirm` is `T_Data.conf`, for the addressing the request
/// was made with, its result mapped as the server side maps it.
#[test]
fn a_confirm_is_a_data_conf_for_the_request_as_made() {
    let secure = Ai {
        mtype: Mtype::SecureDiag,
        ..TO_ENTITY
    };
    let mut t = transport([Entity::Acks(DoIpResult::TimeoutA)]);
    send(&mut t, secure, READ);
    assert_eq!(
        next(&mut t),
        Seen::Conf(secure, failed(DoIpResult::TimeoutA))
    );
}

/// A request the connection refuses gets no confirm from it, so the transport confirms
/// it failed itself, by the next event.
#[test]
fn a_refused_request_is_confirmed_failed_by_the_next_event() {
    let mut t = transport([]);
    run(t.t_data_req(TO_ENTITY, &[], AfterSend::Continue)).unwrap();
    assert_eq!(
        next(&mut t),
        Seen::Conf(TO_ENTITY, failed(DoIpResult::Error))
    );
    run(t.t_data_req(TO_ENTITY, &[0x2E; MAX_PDU + 1], AfterSend::Continue)).unwrap();
    assert_eq!(
        next(&mut t),
        Seen::Conf(TO_ENTITY, failed(DoIpResult::OutOfMemory))
    );
}

/// The tester carries one request at a time; a second, a keep-alive beside a call's
/// request, waits behind the first, and each is confirmed in the order it was made.
#[test]
fn a_second_request_waits_for_the_first_ones_confirm() {
    let group = ai(LogicalAddress(0xE400), uds_session::TaType::Functional);
    let mut t = transport([Entity::Acks(DoIpResult::Ok), Entity::Acks(DoIpResult::Ok)]);
    send(&mut t, TO_ENTITY, READ);
    send(&mut t, group, KEEP_ALIVE);
    assert_eq!(t.connection().requests(), [READ.to_vec()]);

    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
    assert_eq!(next(&mut t), Seen::Conf(group, SResult::Ok));
    assert_eq!(
        t.connection().requests(),
        [READ.to_vec(), KEEP_ALIVE.to_vec()]
    );
}

/// A request waiting behind another when the connection ends is confirmed failed, once.
#[test]
fn a_waiting_request_is_confirmed_failed_when_the_connection_ends() {
    let group = ai(LogicalAddress(0xE400), uds_session::TaType::Functional);
    let mut t = transport([Entity::Closes]);
    send(&mut t, TO_ENTITY, READ);
    send(&mut t, group, KEEP_ALIVE);

    assert_eq!(
        next(&mut t),
        Seen::Conf(TO_ENTITY, failed(DoIpResult::NoSocket))
    );
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));
    assert_eq!(
        next(&mut t),
        Seen::Conf(group, failed(DoIpResult::NoSocket))
    );
    assert_eq!(
        next_until(&mut t, Some(uds_session::Timestamp(100))),
        Seen::Deadline
    );
    assert_eq!(t.connection().requests(), [READ.to_vec()]);
}

/// The transport's limits and clock are the connection's; its reloads are the ones it was
/// built with.
#[test]
fn limits_timing_and_time_come_from_the_connection() {
    let mut connection = Scripted::new([]);
    connection.now = 1_234;
    let t: Transport = DoIpClientTransport::new(connection, RELOADS);
    assert_eq!(<Transport as UdsTransport>::MAX_PDU, MAX_PDU);
    assert_eq!(t.channel_timing(), RELOADS);
    assert_eq!(t.now(), uds_session::Timestamp(1_234));
    assert_eq!(t.outbound_max(), None);
}

/// Ending the session closes the connection, and reports no `Closed` for it.
#[test]
fn close_closes_the_connection_and_reports_no_close() {
    let mut t = transport([]);
    run(ClientTransport::close(&mut t)).unwrap();
    assert_eq!(t.connection().calls, [Call::Close]);
    assert_eq!(
        next_until(&mut t, Some(uds_session::Timestamp(100))),
        Seen::Deadline
    );
}

// --- closes and reconnecting ------------------------------------------------------------

const ENTITY_2: LogicalAddress = LogicalAddress(0x0002);
const TO_ENTITY_2: Ai = ai(ENTITY_2, uds_session::TaType::Physical);

/// A connection's end is one `Closed` for each server addressed on it, then nothing but
/// the deadline: the transport neither repeats it nor spins on the connection.
#[test]
fn an_end_is_one_closed_per_server_then_quiet() {
    let mut t = transport([
        Entity::Acks(DoIpResult::Ok),
        Entity::Acks(DoIpResult::Ok),
        Entity::Closes,
    ]);
    send(&mut t, TO_ENTITY, READ);
    send(&mut t, TO_ENTITY_2, READ);
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY_2, SResult::Ok));

    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY_2.0), false));
    assert_eq!(
        next_until(&mut t, Some(uds_session::Timestamp(100))),
        Seen::Deadline
    );
    assert_eq!(t.connection().quiet_calls, 1);
}

/// A connection no request was addressed on names no server, so its end is reported by
/// no `Closed`: nothing waits on it.
#[test]
fn an_end_with_no_server_addressed_reports_nothing() {
    let mut t = transport([Entity::Closes]);
    assert_eq!(
        next_until(&mut t, Some(uds_session::Timestamp(100))),
        Seen::Deadline
    );
}

/// ISO 14229-5:2022 REQ 7.9 and REQ 7.11: a server closes the connection after a
/// positive `DiagnosticSessionControl` response that leaves its software, and after every
/// positive `ECUReset` response, so a close following either is the one the standard
/// prescribes.
#[test]
fn a_close_after_a_session_change_or_reset_is_expected() {
    for response in [&[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4][..], &[0x51, 0x01]] {
        let mut t = transport([
            Entity::Acks(DoIpResult::Ok),
            answer(response),
            Entity::Closes,
        ]);
        send(&mut t, TO_ENTITY, &[response[0] - 0x40, response[1]]);
        assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
        assert_eq!(next(&mut t), Seen::Ind(FROM_ENTITY, response.to_vec()));
        assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), true));
    }
}

/// A server that answers again after its positive response has not closed for it, so a
/// later end is not the prescribed one.
#[test]
fn a_close_after_a_later_answer_is_not_expected() {
    let mut t = transport([
        Entity::Acks(DoIpResult::Ok),
        answer(&[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
        Entity::Acks(DoIpResult::Ok),
        answer(ANSWER),
        Entity::Closes,
    ]);
    send(&mut t, TO_ENTITY, &[0x10, 0x03]);
    let _ = (next(&mut t), next(&mut t));
    send(&mut t, TO_ENTITY, READ);
    let _ = (next(&mut t), next(&mut t));
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));
}

/// ISO 14229-5:2022 REQ 7.8 and REQ 7.10: after a close, a new connection and routing
/// activation come before diagnostic communication continues. The transport reconnects
/// for the next request, and only then sends it.
#[test]
fn a_request_after_a_close_reconnects_first() {
    let mut t = transport([
        Entity::Acks(DoIpResult::Ok),
        Entity::Closes,
        Entity::Acks(DoIpResult::Ok),
    ]);
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));

    send(&mut t, TO_ENTITY, READ);
    assert_eq!(
        t.connection().calls[1..],
        [
            Call::Reconnect,
            Call::Request(ENTITY, TaType::Physical, READ.to_vec())
        ]
    );
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
}

/// A connection that ended before its `Closed` was read is found closed by the request:
/// the transport reconnects and sends, and reports the end it found before anything from
/// the new connection, so the client hears of it and its request still goes.
#[test]
fn a_request_finding_the_connection_ended_reconnects_and_reports_the_end() {
    let mut connection = Scripted::new([Entity::Acks(DoIpResult::Ok)]);
    connection.connected = false;
    let mut t: Transport = DoIpClientTransport::new(connection, RELOADS);

    send(&mut t, TO_ENTITY, READ);
    assert_eq!(
        t.connection().calls,
        [
            Call::Reconnect,
            Call::Request(ENTITY, TaType::Physical, READ.to_vec())
        ]
    );
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
}

/// A reconnect that fails refuses the request with its error, and leaves the connection
/// closed for a later request to try again.
#[test]
fn a_failed_reconnect_refuses_the_request_and_the_next_tries_again() {
    let mut connection = Scripted::new([Entity::Acks(DoIpResult::Ok)]);
    connection.connected = false;
    connection.failing_reconnects = 1;
    let mut t: Transport = DoIpClientTransport::new(connection, RELOADS);

    assert!(matches!(
        run(t.t_data_req(TO_ENTITY, READ, AfterSend::Continue)),
        Err(uds_on_ip::ClientTransportError::Reconnect("refused"))
    ));
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(
        t.connection().calls,
        [
            Call::Reconnect,
            Call::Reconnect,
            Call::Request(ENTITY, TaType::Physical, READ.to_vec())
        ]
    );
}

/// A request none of which was written is withdrawn with `DoIP_TIMEOUT_A` and the
/// connection stays up, so `TimeoutA` alone reconnects nothing.
#[test]
fn a_timeout_a_alone_reconnects_nothing() {
    let mut t = transport([
        Entity::Acks(DoIpResult::TimeoutA),
        Entity::Acks(DoIpResult::Ok),
    ]);
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(
        next(&mut t),
        Seen::Conf(TO_ENTITY, failed(DoIpResult::TimeoutA))
    );
    send(&mut t, TO_ENTITY, READ);
    assert!(!t.connection().calls.contains(&Call::Reconnect));
}

// --- a late reply -----------------------------------------------------------------------

fn reconnects(t: &Transport) -> usize {
    t.connection()
        .calls
        .iter()
        .filter(|call| **call == Call::Reconnect)
        .count()
}

/// The open question "Should a reset discard a message already arriving?": a response
/// that arrives after its window closed would be taken for the next request's. A request
/// to a server whose last request was confirmed and never answered goes on a new
/// connection, which the late response cannot reach, and the old one's end is reported.
#[test]
fn a_request_after_an_unanswered_one_goes_on_a_new_connection() {
    let mut t = transport([Entity::Acks(DoIpResult::Ok), Entity::Acks(DoIpResult::Ok)]);
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));

    send(&mut t, TO_ENTITY, READ);
    assert_eq!(
        t.connection().calls[1..],
        [
            Call::Reconnect,
            Call::Request(ENTITY, TaType::Physical, READ.to_vec())
        ]
    );
    assert_eq!(next(&mut t), Seen::Closed(Address(ENTITY.0), false));
    assert_eq!(next(&mut t), Seen::Conf(TO_ENTITY, SResult::Ok));
}

/// An answered request leaves nothing to arrive late.
#[test]
fn a_request_after_an_answered_one_keeps_the_connection() {
    let mut t = transport([Entity::Acks(DoIpResult::Ok), answer(ANSWER)]);
    send(&mut t, TO_ENTITY, READ);
    let _ = (next(&mut t), next(&mut t));
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(reconnects(&t), 0);
}

/// A response-pending message is not the answer: the final response may still come late.
#[test]
fn a_request_after_only_a_response_pending_goes_on_a_new_connection() {
    let mut t = transport([Entity::Acks(DoIpResult::Ok), answer(&[0x7F, 0x22, 0x78])]);
    send(&mut t, TO_ENTITY, READ);
    let _ = (next(&mut t), next(&mut t));
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(reconnects(&t), 1);
}

/// A request whose positive response is suppressed (ISO 14229-1:2020 9.2.2's SPRMIB)
/// expects none, so it neither leaves a reply to arrive late nor, sent while a reply is
/// awaited, gives up the connection that reply arrives on.
#[test]
fn a_suppressed_request_neither_arms_nor_triggers_the_guard() {
    let mut t = transport([
        Entity::Acks(DoIpResult::Ok),
        Entity::Acks(DoIpResult::Ok),
        Entity::Acks(DoIpResult::Ok),
        Entity::Acks(DoIpResult::Ok),
    ]);
    send(&mut t, TO_ENTITY, KEEP_ALIVE);
    let _ = next(&mut t);
    send(&mut t, TO_ENTITY, &[0x10, 0x81]);
    let _ = next(&mut t);
    send(&mut t, TO_ENTITY, READ);
    let _ = next(&mut t);
    send(&mut t, TO_ENTITY, KEEP_ALIVE);
    assert_eq!(reconnects(&t), 0);
}

/// A request the server never saw, its acknowledgement negative, has no reply to come.
#[test]
fn a_request_after_a_refused_one_keeps_the_connection() {
    let mut t = transport([Entity::Acks(DoIpResult::UnknownTa)]);
    send(&mut t, TO_ENTITY, READ);
    let _ = next(&mut t);
    send(&mut t, TO_ENTITY, READ);
    assert_eq!(reconnects(&t), 0);
}

/// A functional request's answers end with its window, not with an answer, so it arms
/// no guard: fanning out is not supported, and the client's own service check stands.
#[test]
fn a_functional_request_arms_no_guard() {
    let group = ai(LogicalAddress(0xE400), uds_session::TaType::Functional);
    let mut t = transport([Entity::Acks(DoIpResult::Ok)]);
    send(&mut t, group, READ);
    let _ = next(&mut t);
    send(&mut t, group, READ);
    assert_eq!(reconnects(&t), 0);
}

// --- periodic responses -----------------------------------------------------------------

const PERIODIC: u16 = 0x8004;

/// ISO 14229-5:2022 REQ 7.16: a periodic response is its own payload type, `0x8004`,
/// formatted as a diagnostic message: addresses, then the periodic data identifier and
/// its record. REQ 7.20 is why it is `Periodic` and never `DataInd`.
#[test]
fn a_periodic_response_to_the_tester_is_periodic() {
    let mut t = transport([Entity::SendsUnmodelled(
        PERIODIC,
        vec![0x00, 0x01, 0x0E, 0x00, 0xF2, 0x01, 0x02],
    )]);
    assert_eq!(
        next(&mut t),
        Seen::Periodic(FROM_ENTITY, 0xF2, vec![0x01, 0x02])
    );
}

/// What cannot be a whole periodic response to this tester is dropped: one to another
/// tester, one too short to carry an identifier, one truncated, and any other payload
/// type.
#[test]
fn what_is_not_a_whole_periodic_response_to_the_tester_is_dropped() {
    let mut t = transport([
        Entity::SendsUnmodelled(PERIODIC, vec![0x00, 0x01, 0x0E, 0x01, 0xF2, 0x01]),
        Entity::SendsUnmodelled(PERIODIC, vec![0x00, 0x01, 0x0E, 0x00]),
        Entity::SendsUnmodelledLong(PERIODIC, vec![0x00, 0x01, 0x0E, 0x00, 0xF2], 900),
        Entity::SendsUnmodelled(0x8005, vec![0x00, 0x01, 0x0E, 0x00, 0xF2]),
        answer(ANSWER),
    ]);
    assert_eq!(next(&mut t), Seen::Ind(FROM_ENTITY, ANSWER.to_vec()));
}

// --- the entity's maximum data size -----------------------------------------------------

/// ISO 13400-2:2019 Table 11's *Max. data size* counts a diagnostic message's payload,
/// its two addresses included; what a UDS message may take is the rest.
#[test]
fn outbound_max_is_the_max_data_size_less_the_addresses() {
    let t = |mds| transport([]).with_max_data_size(mds).outbound_max();
    assert_eq!(transport([]).outbound_max(), None);
    assert_eq!(t(None), None);
    assert_eq!(t(Some(4_092)), Some(4_088));
    assert_eq!(t(Some(2)), Some(0));
}
