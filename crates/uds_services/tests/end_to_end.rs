//! Milestone 1's "done when" (spec §3), through `Server::step` over a scripted transport.

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

fn from_tester(ta_type: TaType) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: TESTER,
        ta: ECU,
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
    /// Advance the clock; `next_event` reports `Deadline`.
    At(u32),
}

const MAX_STEPS: usize = 16;
const MAX_SENT: usize = 8;
const MAX_FRAME: usize = 32;

#[derive(Debug)]
struct Scripted {
    script: [Option<Ev>; MAX_STEPS],
    cursor: usize,
    now: u32,
    sent: [([u8; MAX_FRAME], usize); MAX_SENT],
    /// For each transmission, how many script steps had been consumed when it was made.
    sent_after: [usize; MAX_SENT],
    sent_count: usize,
}

impl Scripted {
    fn new(script: &[Ev]) -> Self {
        let mut s = [None; MAX_STEPS];
        for (slot, ev) in s.iter_mut().zip(script) {
            *slot = Some(*ev);
        }
        Self {
            script: s,
            cursor: 0,
            now: 0,
            sent: [([0; MAX_FRAME], 0); MAX_SENT],
            sent_after: [0; MAX_SENT],
            sent_count: 0,
        }
    }
    fn sent(&self, i: usize) -> &[u8] {
        self.sent
            .get(i)
            .and_then(|(buf, n)| buf.get(..*n))
            .unwrap_or(&[])
    }
    fn record(&mut self, data: &[u8]) -> Result<(), ()> {
        let (buf, n) = self.sent.get_mut(self.sent_count).ok_or(())?;
        buf.get_mut(..data.len()).ok_or(())?.copy_from_slice(data);
        *n = data.len();
        *self.sent_after.get_mut(self.sent_count).ok_or(())? = self.cursor;
        self.sent_count = self.sent_count.wrapping_add(1);
        Ok(())
    }
    fn advance<'b>(&mut self, buffer: &'b mut [u8]) -> Result<TransportEvent<'b>, ()> {
        let ev = self.script.get(self.cursor).copied().flatten().ok_or(())?;
        self.cursor = self.cursor.wrapping_add(1);
        let ind = |buffer: &'b mut [u8], ta_type, bytes: &[u8]| {
            let (head, _) = buffer.split_at_mut_checked(bytes.len()).ok_or(())?;
            head.copy_from_slice(bytes);
            Ok(TransportEvent::DataInd {
                ai: from_tester(ta_type),
                data: head,
            })
        };
        match ev {
            Ev::Ind(ta_type, bytes) => ind(buffer, ta_type, bytes),
            Ev::IndAt(t, ta_type, bytes) => {
                self.now = t;
                ind(buffer, ta_type, bytes)
            }
            Ev::Conf(result) => Ok(TransportEvent::DataConf {
                ai: to_tester(),
                result,
            }),
            Ev::At(t) => {
                self.now = t;
                Ok(TransportEvent::Deadline)
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
        ready(self.record(data))
    }
    fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        _deadline: Option<Timestamp>,
    ) -> impl Future<Output = Result<TransportEvent<'b>, ()>> {
        let mut parts = Some((self, buffer));
        poll_fn(move |_| {
            Poll::Ready(parts.take().ok_or(()).and_then(|(t, b)| t.advance(b)))
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

/// Pends `0.0` times before completing, waking itself each time.
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
}
impl Ecu {
    const fn new() -> Self {
        Self {
            transitions: [None; 4],
            n: 0,
            slow: 0,
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
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}
impl DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
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
    peers = 2,
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

/// Run until the script is exhausted (the transport then errors).
fn run(server: &mut EcuServer) {
    while block_on(server.step()).is_ok() {}
}

#[test]
fn a_physical_read_is_answered() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x22, 0xF4, 0x0D]),
            Ev::Conf(SResult::Ok),
        ]),
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
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x11, 0x11]);
    assert_eq!(s.transport().sent_count, 1);
}

#[test]
fn a_short_read_gets_0x13() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[Ev::Ind(TaType::Physical, &[0x22, 0xF4])]),
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
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x22, 0x78]);
    assert_eq!(s.transport().sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
    assert_eq!(s.transport().sent_count, 2);
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
/// stage settles one of the five silenced codes is answered anyway.
#[test]
fn a_sent_response_pending_overrides_functional_silence() {
    let mut ecu = Ecu::new();
    ecu.slow = 2;
    let mut s = EcuServer::new(
        ecu,
        Scripted::new(&[
            // second DID unknown: skipped, still positive
            Ev::Ind(TaType::Functional, &[0x22, 0xF4, 0x0D, 0x00, 0x01]),
            Ev::At(50),
            Ev::Conf(SResult::Ok),
            Ev::Conf(SResult::Ok),
        ]),
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent(0), &[0x7F, 0x22, 0x78]);
    assert_eq!(s.transport().sent(1), &[0x62, 0xF4, 0x0D, 0x40]);
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
        PARAMS,
    );
    run(&mut s);
    let t = s.transport();
    assert_eq!(t.sent(0), &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(t.sent(1), &[0x7F, 0x27, 0x11]); // allowed in extended; listed, no stage
    assert_eq!(t.sent(2), &[0x7F, 0x27, 0x7F]); // Table 23 refuses it in default
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

/// Without the completion report the request's arrival would leave `tS3_Server` stopped
/// forever; with it, the session times out 5000 ms after the keep-alive completed.
#[test]
fn a_suppressed_keep_alive_restarts_the_session_timer() {
    let mut s = EcuServer::new(
        Ecu::new(),
        Scripted::new(&[
            Ev::Ind(TaType::Physical, &[0x10, 0x03]),
            Ev::Conf(SResult::Ok),
            Ev::IndAt(4_000, TaType::Physical, &[0x3E, 0x80]),
            Ev::At(8_000),
            Ev::At(9_100),
        ]),
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.transport().sent_count, 1); // the 3E 80 was suppressed
    assert_eq!(
        s.services().transitions.get(1).copied().flatten(),
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
        PARAMS,
    );
    run(&mut s);
    assert_eq!(
        s.services().transitions.get(1).copied().flatten(),
        Some(SessionTransition::NonDefaultToDefault)
    );
    assert_eq!(s.transport().sent(1), &[0x7F, 0x27, 0x7F]);
}

/// Review focus 5 — a failed transmission of the session response changes nothing: the
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
        PARAMS,
    );
    run(&mut s);
    assert_eq!(s.services().transitions.first().copied().flatten(), None);
    assert_eq!(s.transport().sent(1), &[0x7F, 0x27, 0x7F]);
}

/// Spec §3.5 — the driver's future is `Send` for a `Send` transport and service set, and
/// its size is reported so a bare-metal stack budget has a number.
#[test]
fn the_step_future_is_send_and_its_size_is_known() {
    fn assert_send<F: Send>(_: &F) {}
    let mut s = EcuServer::new(Ecu::new(), Scripted::new(&[]), PARAMS);
    let fut = s.step();
    assert_send(&fut);
    let size = core::mem::size_of_val(&fut);
    assert!(size < 4_096, "step future is {size} bytes");
}
