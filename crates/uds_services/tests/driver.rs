//! The driver's arms, against transports that script one situation each.
//!
//! An integration test rather than a unit test in `server.rs`: `uds_server!` cannot be
//! invoked inside this crate, because the helper macros it calls through `$crate::` are
//! themselves macro-expanded `macro_export` macros, which rustc refuses to resolve by an
//! absolute path from their defining crate. The full scripted transport is
//! `end_to_end.rs`'s; these are the cases that need a fixture of their own.

use uds_services::{
    Address, Ai, DiagnosticSessionType as S, Mtype, NegativeResponseCode as Nrc, Reloads,
    ResponseSink, SResult, ServerParams, SessionTiming, SessionTransition, Sink, TaType,
    Timestamp, TransportEvent, UdsTransport, uds_server,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    VehicleSpeed,
}

impl uds_services::DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        0xF4_0D
    }
    fn from_u16(v: u16) -> Option<Self> {
        (v == 0xF4_0D).then_some(Self::VehicleSpeed)
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), uds_services::RecordError> {
        buf.split_at_checked(1)
            .ok_or(uds_services::RecordError::Short)
    }
}

/// The smallest application: one identifier and session control.
#[derive(Debug)]
struct Ecu;

impl uds_services::ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    // A plain `fn` returning a ready future: an `async fn` with no `.await` is
    // `clippy::unused_async_trait_impl`, which pedantic denies.
    fn read(
        &mut self,
        _did: Did,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), Nrc>> {
        core::future::ready(out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong))
    }
}

impl uds_services::DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
    }
    fn supported_from(&self, _s: S, _active: S) -> bool {
        true
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _r: bool) {}
}

/// A transport that confirms a transmission the server never made (``UDSS_LLR_0063``).
#[derive(Debug)]
struct Spurious {
    done: bool,
}

impl UdsTransport for Spurious {
    type Error = ();
    fn t_data_req(
        &mut self,
        _ai: Ai,
        _d: &[u8],
    ) -> impl core::future::Future<Output = Result<(), ()>> {
        core::future::ready(Ok(()))
    }
    fn next_event<'b>(
        &mut self,
        _b: &'b mut [u8],
        _d: Option<Timestamp>,
    ) -> impl core::future::Future<Output = Result<TransportEvent<'b>, ()>> {
        let event = if self.done {
            Err(())
        } else {
            self.done = true;
            Ok(TransportEvent::DataConf {
                ai: Ai {
                    mtype: Mtype::Diag,
                    sa: Address(0x10),
                    ta: Address(0x0E80),
                    ta_type: TaType::Physical,
                },
                result: SResult::Ok,
            })
        };
        core::future::ready(event)
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
        Timestamp(0)
    }
}

uds_server! {
    Ecu: ReadDataByIdentifier, DiagnosticSessionControl;
    transport = Spurious,
    peers = 1,
    server = Srv,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

#[allow(clippy::panic, reason = "a test harness for futures that never pend")]
fn block_on<F: core::future::Future>(f: F) -> F::Output {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    let mut f = core::pin::pin!(f);
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("the fixtures never pend"),
    }
}

/// A spurious confirmation is rejected by the session layer and the driver carries
/// on: `step` returns `Ok`, not an error and not a panic.
#[test]
fn a_spurious_confirmation_is_survived() {
    let mut server = Srv::new(Ecu, Spurious { done: false }, Address(0x10), PARAMS);
    assert_eq!(block_on(server.step()), Ok(()));
    assert!(server.transport().done);
    assert_eq!(block_on(server.step()), Err(()));
}

const ECU: Address = Address(0x10);
const TESTER: Address = Address(0x0E80);
const OTHER_TESTER: Address = Address(0x0E81);

/// A request from `tester` to the ECU, physically addressed.
const fn request_from(tester: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: tester,
        ta: ECU,
        ta_type: TaType::Physical,
    }
}

/// A response from the ECU to `tester`: what the driver submits, and what its
/// confirmation carries.
const fn response_to(tester: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: ECU,
        ta: tester,
        ta_type: TaType::Physical,
    }
}

/// One scripted step of [`Script`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A request arrives.
    Ind(Ai, &'static [u8]),
    /// A transmission to this addressing is confirmed sent.
    Conf(Ai),
    /// The clock advances to this instant. The deadline is reported only if that reaches
    /// the one the driver asked for; otherwise the next step is taken.
    At(u32),
}

const STEPS: usize = 8;
const FRAMES: usize = 4;
const FRAME: usize = 8;

/// A transport that plays a fixed script and records what was sent, and to whom.
#[derive(Debug)]
struct Script {
    steps: [Option<Step>; STEPS],
    cursor: usize,
    now: u32,
    sent: [(Option<Ai>, [u8; FRAME], usize); FRAMES],
    sent_count: usize,
    /// How many `Deadline`s were reported.
    deadlines: usize,
}

impl Script {
    fn new(script: &[Step]) -> Self {
        let mut steps = [None; STEPS];
        for (slot, step) in steps.iter_mut().zip(script) {
            *slot = Some(*step);
        }
        Self {
            steps,
            cursor: 0,
            now: 0,
            sent: [(None, [0; FRAME], 0); FRAMES],
            sent_count: 0,
            deadlines: 0,
        }
    }

    /// Transmission `i`: its addressing and its bytes.
    fn sent(&self, i: usize) -> (Option<Ai>, &[u8]) {
        self.sent.get(i).map_or((None, &[][..]), |(ai, buf, n)| {
            (*ai, buf.get(..*n).unwrap_or(&[]))
        })
    }

    /// The next step, or the one error this transport returns: the script is exhausted.
    fn advance<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, ()> {
        loop {
            let step = self.steps.get(self.cursor).copied().flatten().ok_or(())?;
            self.cursor = self.cursor.wrapping_add(1);
            match step {
                Step::Ind(ai, bytes) => {
                    let (head, _) = buffer.split_at_mut_checked(bytes.len()).ok_or(())?;
                    head.copy_from_slice(bytes);
                    return Ok(TransportEvent::DataInd { ai, data: head });
                }
                Step::Conf(ai) => {
                    return Ok(TransportEvent::DataConf {
                        ai,
                        result: SResult::Ok,
                    });
                }
                Step::At(t) => {
                    self.now = t;
                    // The comparison the seam doc prescribes, right across the wrap.
                    if deadline.is_some_and(|d| Timestamp(t).has_reached(d)) {
                        self.deadlines = self.deadlines.wrapping_add(1);
                        return Ok(TransportEvent::Deadline);
                    }
                }
            }
        }
    }
}

impl UdsTransport for Script {
    type Error = ();
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
    ) -> impl core::future::Future<Output = Result<(), ()>> {
        // A frame that does not fit is recorded with no bytes, so a test comparing them
        // fails rather than passing on a truncation.
        if let Some((slot_ai, buf, n)) = self.sent.get_mut(self.sent_count) {
            *slot_ai = Some(ai);
            if let Some(head) = buf.get_mut(..data.len()) {
                head.copy_from_slice(data);
                *n = data.len();
            }
        }
        self.sent_count = self.sent_count.wrapping_add(1);
        core::future::ready(Ok(()))
    }
    // Lazy, as `end_to_end.rs`'s is: the driver drops this future unpolled whenever the
    // handler wins its `select2`, and a step taken on creation would then be lost.
    fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> impl core::future::Future<Output = Result<TransportEvent<'b>, ()>> {
        let mut parts = Some((self, buffer));
        core::future::poll_fn(move |_| {
            core::task::Poll::Ready(
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

/// Pends `self.0` times before completing, waking itself each time.
#[derive(Debug)]
struct PendN(u8);

impl core::future::Future for PendN {
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

/// An application whose one service may be slow and never admits a response-pending.
#[derive(Debug)]
struct Slow {
    /// How many times the next `read` pends before answering.
    pends: u8,
}

impl uds_services::ReadDataByIdentifier for Slow {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.pends)).await;
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}

uds_server! {
    Slow: ReadDataByIdentifier;
    transport = Script,
    peers = 1,
    server = SlowSrv,
}

/// Step until the transport errors, which it does only once the script is exhausted.
/// Bounded, so a driver that stopped consuming the script fails rather than hangs.
fn run(server: &mut SlowSrv) {
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    for _ in 0..STEPS {
        let mut step = core::pin::pin!(server.step());
        let mut result = None;
        for _ in 0..16 {
            if let core::task::Poll::Ready(r) = step.as_mut().poll(&mut cx) {
                result = Some(r);
                break;
            }
        }
        if result != Some(Ok(())) {
            break;
        }
    }
}

const READ: &[u8] = &[0x22, 0xF4, 0x0D];
const POSITIVE: &[u8] = &[0x62, 0xF4, 0x0D, 0x40];

/// ``UDSSVC_ARCH_0032`` — a 0x78 is admissible only where `may_respond_pending` says so,
/// and the driver does not decide. A slow handler of a service declaring
/// `MAY_RESPOND_PENDING = false` overruns `tP2_Server` and gets no response-pending: the
/// tester sees only the final response.
///
/// Here rather than in `end_to_end.rs`: that fixture's only slow handler is its
/// `ReadDataByIdentifier`, whose 0x78 its other tests rely on, and the services it can
/// set the constant false on (`SecurityAccess`, `TesterPresent`) have no stage that can
/// pend.
#[test]
fn a_service_that_may_not_pend_gets_no_response_pending() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::At(50), // tP2_Server reached, mid-handler
            Step::Conf(response_to(TESTER)),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 3, "the script was not consumed");
    // tP2_Server's deadline was asked for and reached, and still no 0x78 went out.
    assert_eq!(t.deadlines, 1);
    assert_eq!(t.sent_count, 1);
    assert_eq!(t.sent(0), (Some(response_to(TESTER)), POSITIVE));
}

/// ``UDSS_LLR_0062`` — a final response refused because no association is free is waited
/// out and resubmitted once. One association: the first tester's response is still
/// unconfirmed when the second tester's request is answered, so that answer is refused
/// until the first confirmation frees the association, which is any confirmation and not
/// the refused addressing's own.
#[test]
fn a_final_response_refused_for_want_of_an_association_is_still_delivered() {
    let mut server = SlowSrv::new(
        Slow { pends: 0 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(OTHER_TESTER), READ),
            Step::Conf(response_to(TESTER)), // frees the one association
            Step::Conf(response_to(OTHER_TESTER)),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.sent(0), (Some(response_to(TESTER)), POSITIVE));
    assert_eq!(t.sent(1), (Some(response_to(OTHER_TESTER)), POSITIVE));
}

/// ``UDSSVC_ARCH_0032`` while a refused final response is waited out: `tP2_Server`
/// passes during the wait, and a service declaring `MAY_RESPOND_PENDING = false` still
/// gets no response-pending. The deadline is drained, and the final response follows the
/// confirmation that frees the association.
///
/// This exercises the guarded arm but cannot tell it from the unguarded one on the wire:
/// a `MAY_RESPOND_PENDING = false` service never sends a 0x78, so the wait is reachable
/// only through `NoAssociationFree`, which would refuse a 0x78 submitted then as well.
#[test]
fn a_service_that_may_not_pend_gets_no_response_pending_while_a_response_waits() {
    let mut server = SlowSrv::new(
        Slow { pends: 0 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(OTHER_TESTER), READ),
            Step::At(50), // the second request's tP2_Server, mid-wait
            Step::Conf(response_to(TESTER)),
            Step::Conf(response_to(OTHER_TESTER)),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 5, "the script was not consumed");
    // The second request's tP2_Server deadline was asked for during the wait.
    assert_eq!(t.deadlines, 1);
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.sent(0), (Some(response_to(TESTER)), POSITIVE));
    assert_eq!(t.sent(1), (Some(response_to(OTHER_TESTER)), POSITIVE));
}
