//! The server end to end, through `Server::step` over a scripted transport.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test harness: scripts are fixed arrays and futures complete in bounded polls"
)]

use core::future::{Future, poll_fn, ready};
use core::task::Poll;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, Ai, DataIdentifier, DiagnosticSessionControl, DiagnosticSessionType as S,
    KeyVerdict, Mtype, ReadDataByIdentifier, RecordError, Reloads, ResponseSink, SResult,
    SecurityAccess, SecurityLevel, SecurityPolicy, ServerParams, SessionTiming,
    SessionTransition, Sink, TaType, TesterPresent, Timestamp, TransportEvent,
    UdsTransport, uds_server,
};
use uds_session::TransportError;

const TESTER: Address = Address(0x0E80);
const ECU: Address = Address(0x0010);
/// The functional group address a functional request is sent to: never the ECU's own,
/// so a response sent from the request's `S_TA` would be caught.
const FUNCTIONAL: Address = Address(0xE400);

fn from_tester(ta_type: TaType) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: TESTER,
        ta: match ta_type {
            TaType::Physical => ECU,
            TaType::Functional => FUNCTIONAL,
        },
        ta_type,
    }
}
fn to_tester() -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: ECU,
        ta: TESTER,
        ta_type: TaType::Physical,
    }
}

/// One scripted step: what `next_event` yields, and the clock it yields it at.
#[derive(Debug, Clone, Copy)]
enum Ev {
    Ind(TaType, &'static [u8]),
    /// An indication arriving with the clock already at `now` — the coinciding case.
    IndAt(u32, TaType, &'static [u8]),
    Conf(SResult),
    /// Advance the clock. `next_event` reports `Deadline` only if that reaches the deadline
    /// the driver asked for; otherwise the time passes with nothing to report, and the
    /// next step is taken, as a real transport would go on waiting.
    At(u32),
}

const MAX_STEPS: usize = 16;
const MAX_SENT: usize = 8;
const MAX_FRAME: usize = 32;

#[derive(Debug)]
struct Scripted {
    script: [Option<Ev>; MAX_STEPS],
    /// How many steps the script has; [`run`] checks every one was consumed.
    len: usize,
    cursor: usize,
    now: u32,
    sent: [([u8; MAX_FRAME], usize); MAX_SENT],
    /// For each transmission, how many script steps had been consumed when it was made.
    sent_after: [usize; MAX_SENT],
    sent_count: usize,
    /// The addressing of the latest transmission.
    sent_ai: Option<Ai>,
    /// How many `Deadline`s were reported: one per `At` that reached the driver's deadline.
    deadlines: usize,
}

impl Scripted {
    fn new(script: &[Ev]) -> Self {
        assert!(
            script.len() <= MAX_STEPS,
            "script has more than {MAX_STEPS} steps"
        );
        let mut s = [None; MAX_STEPS];
        for (slot, ev) in s.iter_mut().zip(script) {
            *slot = Some(*ev);
        }
        Self {
            script: s,
            len: script.len(),
            cursor: 0,
            now: 0,
            sent: [([0; MAX_FRAME], 0); MAX_SENT],
            sent_after: [0; MAX_SENT],
            sent_count: 0,
            sent_ai: None,
            deadlines: 0,
        }
    }
    fn sent(&self, i: usize) -> &[u8] {
        self.sent
            .get(i)
            .and_then(|(buf, n)| buf.get(..*n))
            .unwrap_or(&[])
    }
    /// Record a transmission. Panics rather than erring on overflow: [`run`] reads every
    /// transport error as the end of the script, so an error here would end a run
    /// silently and hide the transmission that caused it.
    fn record(&mut self, data: &[u8]) {
        let index = self.sent_count;
        let Some(((buf, n), after)) =
            self.sent.get_mut(index).zip(self.sent_after.get_mut(index))
        else {
            panic!("transmission {index} ({data:02X?}) exceeds the {MAX_SENT} recorded");
        };
        let Some(head) = buf.get_mut(..data.len()) else {
            panic!("transmission {index} ({data:02X?}) exceeds {MAX_FRAME} bytes");
        };
        head.copy_from_slice(data);
        *n = data.len();
        *after = self.cursor;
        self.sent_count = self.sent_count.wrapping_add(1);
    }
    fn advance<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, ()> {
        let ind = |buffer: &'b mut [u8], ta_type, bytes: &[u8]| {
            let Some((head, _)) = buffer.split_at_mut_checked(bytes.len()) else {
                panic!("indication {bytes:02X?} does not fit the driver's buffer");
            };
            head.copy_from_slice(bytes);
            Ok(TransportEvent::DataInd {
                ai: from_tester(ta_type),
                data: head,
            })
        };
        loop {
            // The one error this transport returns: the script is exhausted.
            let ev = self.script.get(self.cursor).copied().flatten().ok_or(())?;
            self.cursor = self.cursor.wrapping_add(1);
            match ev {
                Ev::Ind(ta_type, bytes) => return ind(buffer, ta_type, bytes),
                Ev::IndAt(t, ta_type, bytes) => {
                    self.now = t;
                    return ind(buffer, ta_type, bytes);
                }
                Ev::Conf(result) => {
                    return Ok(TransportEvent::DataConf {
                        ai: to_tester(),
                        result,
                    });
                }
                Ev::At(t) => {
                    self.now = t;
                    if deadline.is_some_and(|d| Timestamp(t).has_reached(d)) {
                        self.deadlines = self.deadlines.wrapping_add(1);
                        return Ok(TransportEvent::Deadline);
                    }
                }
            }
        }
    }
}

// Neither method is an `async fn`: one with no `.await` is
// `clippy::unused_async_trait_impl`, and one returning an `async` block is
// `clippy::manual_async_fn`. `next_event` must still be lazy: the driver creates a
// `next_event` future and drops it unpolled whenever the handler wins its `select2`, and
// a future that took its script step on creation would lose that step. `t_data_req`'s
// future is always awaited at once, so `ready` serves.
impl UdsTransport for Scripted {
    type Error = ();
    fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> impl Future<Output = Result<(), ()>> {
        assert_eq!(ai, to_tester(), "responses go back to the tester");
        self.sent_ai = Some(ai);
        self.record(data);
        ready(Ok(()))
    }
    fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> impl Future<Output = Result<TransportEvent<'b>, ()>> {
        let mut parts = Some((self, buffer));
        poll_fn(move |_| {
            Poll::Ready(
                parts
                    .take()
                    .ok_or(())
                    .and_then(|(t, b)| t.advance(b, deadline)),
            )
        })
    }
    fn outbound_max(&self) -> Option<usize> {
        None
    }
    fn channel_timing(&self) -> Reloads {
        Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        }
    }
    fn now(&self) -> Timestamp {
        Timestamp(self.now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    Speed,
}
impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        0xF40D
    }
    fn from_u16(v: u16) -> Option<Self> {
        (v == 0xF40D).then_some(Self::Speed)
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        buf.split_at_checked(1).ok_or(RecordError::Short)
    }
}

/// Pends `self.0` times before completing, waking itself each time.
#[derive(Debug)]
struct PendN(u8);
impl Future for PendN {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.0 == 0 {
            return core::task::Poll::Ready(());
        }
        self.0 = self.0.saturating_sub(1);
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
    }
}

#[derive(Debug)]
struct Ecu {
    transitions: [Option<SessionTransition>; 4],
    n: usize,
    /// How many times the next `read` pends before answering.
    slow: u8,
    /// What the next `read` answers once it has pended: its record, or this code.
    refuse: Option<Nrc>,
}
impl Ecu {
    const fn new() -> Self {
        Self {
            transitions: [None; 4],
            n: 0,
            slow: 0,
            refuse: None,
        }
    }
}
impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = 2;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        let pends = core::mem::take(&mut self.slow);
        PendN(pends).await;
        if let Some(code) = self.refuse.take() {
            return Err(code);
        }
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}
impl DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
    }
    /// Programming is entered only from Extended (so `10 02` from Default is 0x7E);
    /// every other session from any.
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        }
    }
    fn on_transition(&mut self, t: SessionTransition, _relocked: bool) {
        if let Some(slot) = self.transitions.get_mut(self.n) {
            *slot = Some(t);
        }
        self.n = self.n.wrapping_add(1);
    }
}
impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}
impl SecurityAccess for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_SEED_LEN: usize = 4;
    const MAX_KEY_LEN: usize = 4;
    fn policy(&self, _l: SecurityLevel) -> SecurityPolicy {
        SecurityPolicy::Counted {
            attempt_limit: 3,
            delay_ms: Some(10_000),
            static_seed: false,
        }
    }
    fn load_attempts(&self, _l: SecurityLevel) -> u8 {
        0
    }
    fn store_attempts(&mut self, _l: SecurityLevel, _c: u8) {}
    fn delay_running(&self, _l: SecurityLevel) -> bool {
        false
    }
    fn start_delay(&mut self, _l: SecurityLevel) {}
    fn seed(
        &mut self,
        _l: SecurityLevel,
        out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        ready(
            out.write_all(&[1, 2, 3, 4])
                .map_err(|_| Nrc::ResponseTooLong),
        )
    }
    fn verify_key(
        &mut self,
        _l: SecurityLevel,
        _key: &[u8],
    ) -> impl Future<Output = Result<KeyVerdict, Nrc>> {
        ready(Ok(KeyVerdict::Invalid))
    }
}

uds_server! {
    Ecu: ReadDataByIdentifier, DiagnosticSessionControl, TesterPresent, SecurityAccess;
    transport = Scripted,
    peers = 1,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
};

/// Poll to completion with a no-op waker; every future here is ready within a bounded
/// number of polls.
fn block_on<F: Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    for _ in 0..64 {
        if let core::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
    panic!("future did not complete in 64 polls");
}

/// Run until the transport errors, which it does only once the script is exhausted, and
/// check that it was: a step left unconsumed means the driver stopped early.
fn run(server: &mut EcuServer) {
    while block_on(server.step()).is_ok() {}
    let t = server.transport();
    assert_eq!(
        t.cursor, t.len,
        "the run ended with script steps unconsumed"
    );
}

#[test]
fn a_physical_read_is_answered() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x62, 0xF4, 0x0D, 0x40]);
}

#[test]
fn an_unsupported_sid_gets_0x11_and_an_empty_request_nothing() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x11, 0x01]),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, &[]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x11, 0x11]);
    assert_eq!(s.transport().sent_count, 1);
}

/// ``UDSSVC_ARCH_0006`` — support is checked before decoding (Figure 5, clause 8.7.5),
/// so a malformed `ECUReset` (no sub-function), which `Ecu` does not list, settles 0x11
/// and not 0x13. Functionally addressed, 0x11 is one of the five silenced codes; the
/// same bytes physically addressed are answered `7F 11 11`.
#[test]
fn a_malformed_request_for_an_unlisted_service_is_0x11_and_silent_when_functional() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Functional, &[0x11]),
            Ev::Ind(TaType::Physical, &[0x11]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 1);
    assert_eq!(t.sent(0), &[0x7F, 0x11, 0x11]);
    // Sent after the physical indication, so the functional one was answered by nothing.
    assert_eq!(t.sent_after.first(), Some(&2));
}

/// ``UDSSVC_ARCH_0007`` — Figure 6 checks the sub-function (0x12) before the exact
/// length (0x13), as clause 8.7.5's pseudo-code does. A reserved `TesterPresent`
/// sub-function with a trailing byte is 0x12: silenced when functionally addressed,
/// `7F 3E 12` when physically addressed. An unsupported session with a trailing byte is
/// `7F 10 12` likewise.
#[test]
fn an_unsupported_sub_function_with_a_trailing_byte_is_0x12_not_0x13() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Functional, &[0x3E, 0x05, 0x00]),
            Ev::Ind(TaType::Physical, &[0x3E, 0x05, 0x00]),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x10, 0x05, 0x00]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.sent(0), &[0x7F, 0x3E, 0x12]);
    // Sent after the physical indication, so the functional one was answered by nothing.
    assert_eq!(t.sent_after.first(), Some(&2));
    assert_eq!(t.sent(1), &[0x7F, 0x10, 0x12]);
}

/// ``UDSSVC_ARCH_0007`` row 4 — Figure 6's "`SubFunction` supported in active session?"
/// answered by `DiagnosticSessionControl::supported_from`. `Ecu` supports Programming
/// only from Extended: `10 02` in Default is `7F 10 7E` physically and silent
/// functionally (``UDSSVC_ARCH_0009`` rule 1); once `10 03` is confirmed, the same `10 02`
/// is answered `50 02` and the session is entered.
#[test]
fn a_session_supported_only_from_another_is_0x7e_until_that_one_is_active() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Functional, &[0x10, 0x02]),
            Ev::Ind(TaType::Physical, &[0x10, 0x02]),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok), // confirms 50 03 ...: extended now
            Ev::Ind(TaType::Physical, &[0x10, 0x02]),
            Ev::Conf(SResult::Ok), // confirms 50 02 ...: programming now
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.sent(0), &[0x7F, 0x10, 0x7E]);
    // Sent after the physical indication, so the functional one was answered by nothing.
    assert_eq!(t.sent_after.first(), Some(&2));
    assert_eq!(t.sent(1), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(2), &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(
        s.services().transitions,
        [
            Some(SessionTransition::DefaultToNonDefault),
            Some(SessionTransition::NonDefaultToNonDefault),
            None,
            None,
        ]
    );
}

#[test]
fn a_short_read_gets_0x13() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[Ev::Ind(TaType::Physical, &[0x22, 0xF4])]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x22, 0x13]);
}

#[test]
fn a_functional_unsupported_did_is_silent() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[Ev::Ind(TaType::Functional, &[0x22, 0x00, 0x01])]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent_count, 0);
}

#[test]
fn a_second_request_is_accepted_after_the_first_is_confirmed() {
    let read: &[u8] = &[0x22, 0xF4, 0x0D];
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, read),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, read),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent_count, 2);
}

/// The handler pends twice: once so the driver sees the deadline and sends 0x78, once so
/// it sees that 0x78's confirmation (which opens the enhanced window) before answering.
#[test]
fn a_slow_handler_gets_a_response_pending_then_its_answer() {
    let mut ecu = Ecu::new();
    ecu.slow = 2;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::At(50),            // tP2_Server reached: 0x78 goes out
            Ev::Conf(SResult::Ok), // its confirmation, consumed mid-handler
            Ev::Conf(SResult::Ok), // the final response's confirmation
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x22, 0x78]);
    assert_eq!(s.transport().sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(s.transport().sent_count, 2);
    // The driver asked for tP2_Server's deadline, and At(50) reached it.
    assert_eq!(s.transport().deadlines, 1);
}

/// ``UDSS_LLR_0061`` — the handler pends once, so the driver sees the deadline and sends
/// 0x78, and then completes on the very next poll. The driver's `select2` polls the
/// handler before the transport, so the handler wins that poll and the 0x78's `DataConf`
/// (the next scripted step) has not been drained when the final response is submitted.
/// The session layer refuses it with `AssociationOutstanding`; the driver waits out the
/// confirmation and resubmits once. The tester gets the final response, after the
/// `DataConf`, exactly once.
#[test]
fn a_final_response_after_a_pending_whose_confirmation_is_late_is_still_delivered() {
    let mut ecu = Ecu::new();
    ecu.slow = 1;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::At(50), // tP2_Server reached: 0x78 goes out; handler then done
            Ev::Conf(SResult::Ok), // the 0x78's confirmation, drained after the handler
            Ev::Conf(SResult::Ok), // the final response's confirmation
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent(0), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent_count, 2);
    // Indication, deadline and the 0x78's confirmation consumed before the final went out.
    assert_eq!(t.sent_after.get(1), Some(&3));
}

/// ``UDSSVC_ARCH_0009`` rule 3 — after a sent 0x78, a functionally addressed request whose
/// stage settles one of the five silenced codes is answered anyway. The handler pends past
/// `tP2_Server` and then refuses with `requestOutOfRange` (0x31), which rule 1 alone would
/// silence: the tester, having seen `7F 22 78`, gets `7F 22 31` rather than waiting out
/// `P2*_Client` for nothing (clause 8.7.5).
#[test]
fn a_sent_response_pending_overrides_functional_silence() {
    let mut ecu = Ecu::new();
    ecu.slow = 2;
    ecu.refuse = Some(Nrc::RequestOutOfRange);
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Functional, &[0x22, 0xF4, 0x0D]),
            Ev::At(50),            // tP2_Server reached: 0x78 goes out
            Ev::Conf(SResult::Ok), // its confirmation, consumed mid-handler
            Ev::Conf(SResult::Ok), // the final response's confirmation
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.sent(0), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(1), &[0x7F, 0x22, 0x31]);
    // UDSS_LLR_0051 — sent from the ECU's own address, not the functional group's.
    let ai = t.sent_ai.unwrap();
    assert_eq!((ai.sa, ai.ta, ai.ta_type), (ECU, TESTER, TaType::Physical));
}

/// Rule 1 without rule 3: the same refusal, functionally addressed, from a handler that
/// answers within `tP2_Server`, is silenced. Beside the test above, this is what shows
/// the 0x78 is what lifted the silence.
#[test]
fn without_a_response_pending_the_functional_refusal_is_silent() {
    let mut ecu = Ecu::new();
    ecu.refuse = Some(Nrc::RequestOutOfRange);
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[Ev::Ind(TaType::Functional, &[0x22, 0xF4, 0x0D])]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent_count, 0);
}

#[test]
fn a_session_change_takes_effect_on_confirmation_and_times_out() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok), // confirms 50 03 ...: extended now
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
            Ev::Conf(SResult::Ok),
            Ev::At(5_100), // past tS3_Server: default again
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x7F, 0x27, 0x11]); // allowed in extended; listed, no stage
    assert_eq!(t.sent(2), &[0x7F, 0x27, 0x7F]); // Table 23 refuses it in default
    assert_eq!(t.deadlines, 1); // tS3_Server's, reached at 5100
    assert_eq!(
        s.services().transitions,
        [
            Some(SessionTransition::DefaultToNonDefault),
            Some(SessionTransition::NonDefaultToDefault),
            None,
            None,
        ]
    );
}

#[test]
fn a_suppressed_session_change_enters_the_session() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x83]),
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent_count, 1);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x27, 0x11]); // in extended
    assert_eq!(
        s.services().transitions.first().copied().flatten(),
        Some(SessionTransition::DefaultToNonDefault)
    );
}

/// The keep-alive script, up to and including `tail` — the instant the clock is last
/// advanced to.
fn keep_alive_server(tail: u32) -> EcuServer {
    EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            // Extended: tS3_Server runs from 0 and, unless stopped, expires at 5000.
            Ev::Conf(SResult::Ok),
            Ev::IndAt(4_000, TaType::Physical, &[0x3E, 0x80]),
            Ev::At(tail),
        ]),
        ECU,
        PARAMS,
    )
}

/// The suppressed `3E 80` at 4000 ms stopped `tS3_Server` on arrival and its completion
/// report restarted it at 4000 ms, so the session expires at exactly 9000 ms. That is
/// pinned from both sides, by two runs of the same script:
/// - The first run ends at 8999 ms with no expiry. A timer never stopped would have
///   expired at 5000 ms, and one restarted any earlier than 4000 ms would have expired
///   before 8999 ms.
/// - The second run ends at 9000 ms, and the expiry has been reported. A timer stopped and
///   never restarted would never expire.
#[test]
fn a_suppressed_keep_alive_restarts_the_session_timer() {
    let mut before = keep_alive_server(8_999);
    run(&mut before);
    assert_eq!(before.transport().sent_count, 1); // the 3E 80 was suppressed
    // The driver's deadline is tS3_Server's 9000, which 8999 does not reach.
    assert_eq!(before.transport().deadlines, 0);
    assert_eq!(
        before.services().transitions.get(1).copied().flatten(),
        None
    );

    let mut at = keep_alive_server(9_000);
    run(&mut at);
    assert_eq!(at.transport().sent_count, 1);
    assert_eq!(at.transport().deadlines, 1);
    assert_eq!(
        at.services().transitions.get(1).copied().flatten(),
        Some(SessionTransition::NonDefaultToDefault)
    );
}

/// ``UDSS_LLR_0081`` — the expiry is reported before the request that arrived at it, and
/// the request is answered in the default session.
#[test]
fn a_timeout_coinciding_with_a_request_is_reported_first() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok),
            Ev::IndAt(5_000, TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(
        s.services().transitions.get(1).copied().flatten(),
        Some(SessionTransition::NonDefaultToDefault)
    );
    assert_eq!(s.transport().sent(1), &[0x7F, 0x27, 0x7F]);
}

/// ``UDSS_LLR_0085`` — a failed transmission of the session response changes nothing: the
/// server is still in the default session, and the application was not told otherwise.
#[test]
fn a_failed_session_response_selects_nothing() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Transport(TransportError(1))),
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.services().transitions.first().copied().flatten(), None);
    assert_eq!(s.transport().sent(1), &[0x7F, 0x27, 0x7F]);
}

/// The driver's future is `Send` for a `Send` transport and service set, and
/// its size is reported so a bare-metal stack budget has a number.
#[test]
fn the_step_future_is_send_and_its_size_is_known() {
    fn assert_send<F: Send>(_: &F) {}
    let mut s = EcuServer::new(Ecu::new(), Scripted::new(&[]), ECU, PARAMS);
    let fut = s.step();
    assert_send(&fut);
    let size = core::mem::size_of_val(&fut);
    assert!(size < 4_096, "step future is {size} bytes");
}
