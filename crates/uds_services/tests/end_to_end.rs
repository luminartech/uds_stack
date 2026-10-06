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
    Address, AfterSend, Ai, DataIdentifier, Delay, DiagnosticSessionControl,
    DiagnosticSessionType as S, KeyVerdict, Mtype, ReadDataByIdentifier, RecordError,
    Reloads, ResponseSink, SResult, SecurityAccess, SecurityLevel, SecurityPolicy,
    ServerParams, SessionTiming, SessionTransition, Sessions, Sink, TaType, TesterPresent,
    Timestamp, TransportEvent, UdsTransport, uds_server,
};
use uds_session::TransportError;

const TESTER: Address = Address(0x0E80);
const ECU: Address = Address(0x0010);
/// The functional group address a functional request is sent to: never the ECU's own,
/// so a response sent from the request's `S_TA` would be caught.
const FUNCTIONAL: Address = Address(0xE400);
/// A second tester, which never controls the session: its requests leave `tS3_Server`
/// running (``UDSS_LLR_0097``).
const OTHER: Address = Address(0x0E81);
/// The answer to `27 01` wherever Table 23 allows it: the fixture's seed, so a session
/// probe reads as positive in a non-default session and 0x7F in the default one.
const SEED: [u8; 6] = [0x67, 0x01, 0x01, 0x02, 0x03, 0x04];

fn from(sa: Address, ta_type: TaType) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa,
        ta: match ta_type {
            TaType::Physical => ECU,
            TaType::Functional => FUNCTIONAL,
        },
        ta_type,
    }
}
fn from_tester(ta_type: TaType) -> Ai {
    from(TESTER, ta_type)
}
/// A response's addressing: from the ECU, physically, to `ta`.
fn to(ta: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: ECU,
        ta,
        ta_type: TaType::Physical,
    }
}
fn to_tester() -> Ai {
    to(TESTER)
}

/// One scripted step: what `next_event` yields, and the clock it yields it at.
#[derive(Debug, Clone, Copy)]
enum Ev {
    Ind(TaType, &'static [u8]),
    /// An indication arriving with the clock already at `now` — the coinciding case.
    IndAt(u32, TaType, &'static [u8]),
    Conf(SResult),
    /// A physical indication from tester `sa`, arriving with the clock already at `now`.
    IndFrom(u32, Address, &'static [u8]),
    /// The confirmation of the response sent to tester `ta`.
    ConfTo(Address, SResult),
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
    /// Each transmission's addressing.
    sent_ai: [Option<Ai>; MAX_SENT],
    /// What each transmission said follows it.
    sent_then: [Option<AfterSend>; MAX_SENT],
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
            sent_ai: [None; MAX_SENT],
            sent_then: [None; MAX_SENT],
            deadlines: 0,
        }
    }
    fn sent(&self, i: usize) -> &[u8] {
        self.sent
            .get(i)
            .and_then(|(buf, n)| buf.get(..*n))
            .unwrap_or(&[])
    }
    fn sent_then(&self, i: usize) -> Option<AfterSend> {
        self.sent_then.get(i).copied().flatten()
    }
    fn sent_ai(&self, i: usize) -> Option<Ai> {
        self.sent_ai.get(i).copied().flatten()
    }
    /// Record a transmission. Panics rather than erring on overflow: [`run`] reads every
    /// transport error as the end of the script, so an error here would end a run
    /// silently and hide the transmission that caused it.
    fn record(&mut self, ai: Ai, data: &[u8]) {
        let index = self.sent_count;
        let Some((((buf, n), after), sent_ai)) = self
            .sent
            .get_mut(index)
            .zip(self.sent_after.get_mut(index))
            .zip(self.sent_ai.get_mut(index))
        else {
            panic!("transmission {index} ({data:02X?}) exceeds the {MAX_SENT} recorded");
        };
        let Some(head) = buf.get_mut(..data.len()) else {
            panic!("transmission {index} ({data:02X?}) exceeds {MAX_FRAME} bytes");
        };
        head.copy_from_slice(data);
        *n = data.len();
        *after = self.cursor;
        *sent_ai = Some(ai);
        self.sent_count = self.sent_count.wrapping_add(1);
    }
    fn advance<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, ()> {
        let ind = |buffer: &'b mut [u8], ai, bytes: &[u8]| {
            let Some((head, _)) = buffer.split_at_mut_checked(bytes.len()) else {
                panic!("indication {bytes:02X?} does not fit the driver's buffer");
            };
            head.copy_from_slice(bytes);
            Ok(TransportEvent::DataInd { ai, data: head })
        };
        loop {
            // The one error this transport returns: the script is exhausted.
            let ev = self.script.get(self.cursor).copied().flatten().ok_or(())?;
            self.cursor = self.cursor.wrapping_add(1);
            match ev {
                Ev::Ind(ta_type, bytes) => return ind(buffer, from_tester(ta_type), bytes),
                Ev::IndAt(t, ta_type, bytes) => {
                    self.now = t;
                    return ind(buffer, from_tester(ta_type), bytes);
                }
                Ev::IndFrom(t, sa, bytes) => {
                    self.now = t;
                    return ind(buffer, from(sa, TaType::Physical), bytes);
                }
                Ev::Conf(result) => {
                    return Ok(TransportEvent::DataConf {
                        ai: to_tester(),
                        result,
                    });
                }
                Ev::ConfTo(ta, result) => {
                    return Ok(TransportEvent::DataConf { ai: to(ta), result });
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
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        after: AfterSend,
    ) -> impl Future<Output = Result<(), ()>> {
        if let Some(then) = self.sent_then.get_mut(self.sent_count) {
            *then = Some(after);
        }
        assert!(
            ai == to_tester() || ai == to(OTHER),
            "responses go back to a tester, physically, from the ECU: {ai:?}"
        );
        self.record(ai, data);
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
    /// Every level's stored attempt count.
    attempts: u8,
    /// How many times a delay was started.
    delays_started: u8,
    /// The session whose entry leaves the software this server runs, if any.
    leaves: Option<S>,
}
impl Ecu {
    const fn new() -> Self {
        Self {
            transitions: [None; 4],
            n: 0,
            slow: 0,
            refuse: None,
            attempts: 0,
            delays_started: 0,
            leaves: None,
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
    fn leaves_running_software(&self, s: S) -> bool {
        self.leaves == Some(s)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, t: SessionTransition, _entered: S, _relocked: bool) {
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
    const MAX_RECORD_LEN: usize = 0;
    fn sessions(&self, _l: SecurityLevel) -> Option<Sessions> {
        Some(Sessions::ALL)
    }
    fn preconditions_met(&self, _l: SecurityLevel) -> bool {
        true
    }
    fn policy(&self, _l: SecurityLevel) -> SecurityPolicy {
        SecurityPolicy::Counted {
            attempt_limit: core::num::NonZeroU8::MIN.saturating_add(2),
            delay_ms: Some(10_000),
        }
    }
    fn load_attempts(&self, _l: SecurityLevel) -> u8 {
        self.attempts
    }
    fn store_attempts(&mut self, _l: SecurityLevel, _c: u8) {}
    fn delay(&mut self, _l: SecurityLevel) -> Delay {
        Delay::Idle
    }
    fn start_delay(&mut self, _l: SecurityLevel) {
        self.delays_started = self.delays_started.saturating_add(1);
    }
    fn seed(
        &mut self,
        _l: SecurityLevel,
        _record: &[u8],
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
    response_pending_lead: 0,
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

/// Annex I Table I.2 transition 1 — the server starts an owed delay when it starts, on
/// its first step and only then: each supported level at its attempt limit — here every
/// `requestSeed` value — gets one delay, however many steps follow.
#[test]
fn start_up_starts_an_owed_delay_once() {
    let mut s = EcuServer::new(
        Ecu {
            attempts: 3,
            ..Ecu::new()
        },
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    let levels = (0x01_u8..0x7F).step_by(2).count();
    assert_eq!(s.services().delays_started, 0);
    run(&mut s);
    assert_eq!(usize::from(s.services().delays_started), levels);
}

/// W4 addendum item 3, ISO 14229-5:2022 REQ 7.9 — the final positive response to a
/// `DiagnosticSessionControl` whose session leaves the running software is the one
/// message sent `ServerLeaves`; the response entering a session that does not leave is
/// `Continue`, and so is the negative response to the same leaving session refused from
/// the default one (0x7E).
#[test]
fn only_the_response_entering_a_leaving_session_says_the_server_leaves() {
    let mut s = EcuServer::new(
        Ecu {
            leaves: Some(S::ProgrammingSession),
            ..Ecu::new()
        },
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x02]),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x10, 0x02]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent(0), &[0x7F, 0x10, 0x7E]);
    assert_eq!(t.sent_then(0), Some(AfterSend::Continue));
    assert_eq!(t.sent(1), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent_then(1), Some(AfterSend::Continue));
    assert_eq!(t.sent(2), &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent_then(2), Some(AfterSend::ServerLeaves));
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

/// ``UDSS_LLR_0186`` — with a response-pending lead of 10 ms the driver is woken for
/// `tP2_Server` at 40, not 50, so the 0x78 goes out within the window: `At(39)` passes
/// silently and `At(40)` is the deadline. The final response still follows.
#[test]
fn a_response_pending_lead_sends_the_0x78_within_the_window() {
    let mut ecu = Ecu::new();
    ecu.slow = 2;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::At(39),            // short of the deadline: nothing to report
            Ev::At(40),            // tP2_Server less the lead: 0x78 goes out
            Ev::Conf(SResult::Ok), // its confirmation, consumed mid-handler
            Ev::Conf(SResult::Ok), // the final response's confirmation
        ]),
        ECU,
        ServerParams {
            response_pending_lead: 10,
            ..PARAMS
        },
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent(0), &[0x7F, 0x22, 0x78]);
    // Sent once the indication, At(39) and At(40) had been consumed.
    assert_eq!(t.sent_after.first(), Some(&3));
    assert_eq!(t.sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.deadlines, 1);
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

/// ``UDSS_LLR_0108``, ``UDSS_LLR_0109`` — a slow request from the same tester whose
/// indication precedes the previous response's confirmation keeps its `tP2_Server`, so
/// it still gets its `7F 22 78` and then its answer.
///
/// `3E 00` is answered at once and `7E 00` left unconfirmed; the slow read arrives at
/// 10 ms, so its `tP2_Server` runs to 60 ms. `7E 00`'s `DataConf` arrives mid-handler:
/// it frees the one association (`peers = 1`) and answers nothing, so the deadline the
/// handler is raced against is still 60 ms. It is scripted before that deadline because
/// the 0x78 shares `7E 00`'s addressing and ``UDSS_LLR_0061`` would refuse it while
/// `7E 00` was outstanding; likewise, had it come after the read finished, the final
/// response would have waited for it in `await_confirmation`. The handler pends three
/// times — the late confirmation, the deadline, the 0x78's confirmation — and its
/// final response is accepted at once.
#[test]
fn a_request_before_the_previous_confirmation_keeps_its_response_timer() {
    let mut ecu = Ecu::new();
    ecu.slow = 3;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x3E, 0x00]), // 7E 00 submitted, unconfirmed
            Ev::IndAt(10, TaType::Physical, &[0x22, 0xF4, 0x0D]), // pends three times
            Ev::Conf(SResult::Ok), // confirms 7E 00, mid-handler: tP2_Server still runs
            Ev::At(60),            // the read's tP2_Server reached: 0x78 goes out
            Ev::Conf(SResult::Ok), // the 0x78's confirmation, mid-handler
            Ev::Conf(SResult::Ok), // the final response's confirmation
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.sent(0), &[0x7E, 0x00]);
    assert_eq!(t.sent(1), &[0x7F, 0x22, 0x78]);
    // Sent once the deadline was consumed, after the late confirmation.
    assert_eq!(t.sent_after.get(1), Some(&4));
    assert_eq!(t.sent(2), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.deadlines, 1); // the read's tP2_Server, reached at 60
}

/// ``UDSS_LLR_0088`` — in Extended, a slow request from the controlling tester whose
/// indication precedes the previous response's confirmation keeps the session through
/// a service longer than `tS3_Server`.
///
/// Extended from 0 ms. `3E 00` at 1000 ms is answered at once and `7E 00` left
/// unconfirmed; the slow read at 1010 ms stops `tS3_Server` (``UDSS_LLR_0087``). `7E 00`'s
/// `DataConf` arrives mid-handler, after that stop, so it restarts nothing: were it to
/// restart the timer, the session would expire at 6010 ms, before the read's second 0x78
/// at 6060 ms. The handler pends five times — the late confirmation, the two deadlines
/// and the two 0x78 confirmations — and then answers; `27 01` afterwards is still
/// answered in Extended, and no transition back to default is ever reported.
#[test]
fn a_late_confirmation_does_not_restart_the_session_timer_during_the_next_request() {
    let mut ecu = Ecu::new();
    ecu.slow = 5;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok), // confirms 50 03 ...: extended, tS3 expiring at 5000
            Ev::IndAt(1_000, TaType::Physical, &[0x3E, 0x00]), // 7E 00, unconfirmed
            Ev::IndAt(1_010, TaType::Physical, &[0x22, 0xF4, 0x0D]), // pends five times
            Ev::Conf(SResult::Ok), // confirms 7E 00, mid-handler: tS3 stays stopped
            Ev::At(1_060),         // the read's tP2_Server reached: 0x78 goes out
            Ev::Conf(SResult::Ok), // its confirmation: tP2*_Server due at 6060
            Ev::At(6_060),         // tP2*_Server reached: a second 0x78
            Ev::Conf(SResult::Ok), // its confirmation
            Ev::Conf(SResult::Ok), // the final response's: tS3 restarted
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 6);
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x7E, 0x00]);
    assert_eq!(t.sent(2), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(3), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(4), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent(5), &SEED); // allowed in extended: the seed
    assert_eq!(t.deadlines, 2); // the read's tP2_Server and tP2*_Server only
    assert_eq!(
        s.services().transitions,
        [
            Some(SessionTransition::DefaultToNonDefault),
            None,
            None,
            None,
        ]
    );
}

/// ``UDSS_LLR_0085`` — a slow request whose indication precedes the confirmation of the
/// `DiagnosticSessionControl` response before it keeps the session it selected through a
/// service longer than `tS3_Server`.
///
/// `10 03` at 0 ms is answered and `50 03` left unconfirmed; the slow read arrives at
/// 10 ms, in the default session, so no ``UDSS_LLR_0087`` stop follows. `50 03`'s
/// `DataConf` arrives mid-handler and enters Extended with `tS3_Server` stopped: were it
/// started, the session would expire at 5010 ms, before the read's second 0x78 at 5060 ms.
/// The application hears the transition once, and `27 01` afterwards is still answered in
/// Extended.
#[test]
fn a_late_selection_confirmation_does_not_start_the_session_timer_during_the_next_request()
{
    let mut ecu = Ecu::new();
    ecu.slow = 5;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]), // 50 03, unconfirmed
            Ev::IndAt(10, TaType::Physical, &[0x22, 0xF4, 0x0D]), // pends five times
            Ev::Conf(SResult::Ok), // confirms 50 03, mid-handler: extended, tS3 stopped
            Ev::At(60),            // the read's tP2_Server reached: 0x78 goes out
            Ev::Conf(SResult::Ok), // its confirmation: tP2*_Server due at 5060
            Ev::At(5_060),         // tP2*_Server reached: a second 0x78
            Ev::Conf(SResult::Ok), // its confirmation
            Ev::Conf(SResult::Ok), // the final response's: tS3 started
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 5);
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(2), &[0x7F, 0x22, 0x78]);
    assert_eq!(t.sent(3), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent(4), &SEED); // allowed in extended, so the seed
    assert_eq!(t.deadlines, 2); // the read's tP2_Server and tP2*_Server only
    assert_eq!(
        s.services().transitions,
        [
            Some(SessionTransition::DefaultToNonDefault),
            None,
            None,
            None,
        ]
    );
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
    let ai = t.sent_ai(1).unwrap();
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
    assert_eq!(t.sent(1), &SEED); // allowed in extended: the seed
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
    assert_eq!(s.transport().sent(0), &SEED); // in extended
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

/// ``UDSS_LLR_0100`` — a `tS3_Server` expiry that surfaces while a handler is running is
/// applied once the handler releases the services, and only once. Pins `serve`'s
/// `Deadline` arm for a service that admits a 0x78 (``UDSSVC_ARCH_0032``), whose
/// `answer_overrun` tick drains the `SessionTimeout` and whose `merge` carries it out of
/// `serve` to `apply`.
///
/// TESTER enters Extended at 0 ms, so `tS3_Server` expires at 5000 ms. OTHER's slow read
/// arrives at 4990 ms and leaves the timer running (``UDSS_LLR_0097``). Its `tP2_Server`
/// would expire at 5040 ms, so the deadline the handler is raced against is the session's
/// at 5000 ms: the drain at 5000 ms holds the timeout and no overrun, and no 0x78 goes out.
/// The handler then answers, in the default session, back to OTHER.
#[test]
fn a_session_timeout_during_a_handler_is_applied_once() {
    let mut ecu = Ecu::new();
    ecu.slow = 1;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok), // confirms 50 03 ...: extended, tS3 expiring at 5000
            Ev::IndFrom(4_990, OTHER, &[0x22, 0xF4, 0x0D]), // pends once
            Ev::At(5_000),         // tS3_Server reached mid-handler; tP2_Server (5040) not
            Ev::ConfTo(OTHER, SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent_ai(1), Some(to(OTHER)));
    // Sent after the deadline was consumed: the handler answered after the expiry.
    assert_eq!(t.sent_after.get(1), Some(&4));
    assert_eq!(t.sent(2), &[0x7F, 0x27, 0x7F]); // Table 23 refuses it in default
    assert_eq!(t.sent_ai(2), Some(to_tester()));
    assert_eq!(t.deadlines, 1); // tS3_Server's, reached at 5000
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

/// ``UDSS_LLR_0085`` — a selecting response confirmed while another request's handler
/// is running enters the session once the handler releases the services, and only once.
/// Pins `serve`'s `DataConf` arm, whose drain settles the `Pending` slot and whose `merge`
/// carries the confirmation out of `serve` to `apply`.
///
/// `peers = 1` bounds the associations, not the requests: `50 03` holds the one
/// association until its `DataConf`, but an indication needs none, so OTHER's slow read
/// is received and dispatched meanwhile. The `DataConf` for `50 03` arrives while that
/// handler pends, freeing the association, so the read's final response is accepted at
/// once and `answer` never waits in `await_confirmation`.
#[test]
fn a_session_confirmation_during_a_handler_is_applied_once() {
    let mut ecu = Ecu::new();
    ecu.slow = 1;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]), // 50 03 ... submitted, unconfirmed
            Ev::IndFrom(0, OTHER, &[0x22, 0xF4, 0x0D]), // pends once
            Ev::Conf(SResult::Ok), // confirms 50 03 ... mid-handler: extended now
            Ev::ConfTo(OTHER, SResult::Ok),
            Ev::Ind(TaType::Physical, &[0x27, 0x01]),
            Ev::Conf(SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(t.sent_ai(1), Some(to(OTHER)));
    // Sent after the 50 03's confirmation was consumed, mid-handler.
    assert_eq!(t.sent_after.get(1), Some(&3));
    assert_eq!(t.sent(2), &SEED); // allowed in extended: the seed
    assert_eq!(
        s.services().transitions,
        [
            Some(SessionTransition::DefaultToNonDefault),
            None,
            None,
            None,
        ]
    );
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
