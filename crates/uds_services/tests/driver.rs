//! The driver's arms, against transports that script one situation each.
//!
//! An integration test rather than a unit test in `server.rs`: `uds_server!` cannot be
//! invoked inside this crate, because the helper macros it calls through `$crate::` are
//! themselves macro-expanded `macro_export` macros, which rustc refuses to resolve by an
//! absolute path from their defining crate. The scripted transport is
//! `common`'s, shared with `end_to_end.rs`; `Spurious` is the one fixture of its own.

mod common;

use common::{PendN, STEPS, Script, Step, block_on};
use uds_services::{
    Address, AfterSend, Ai, DiagnosticSessionType as S, Mtype, NegativeResponseCode as Nrc,
    Reloads, ResponseSink, SResult, ServerParams, SessionTiming, SessionTransition, Sink,
    TaType, Timestamp, TransportEvent, UdsTransport, uds_server,
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
    fn leaves_running_software(&self, _s: S) -> bool {
        false
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _entered: S, _r: bool) {}
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
        _after: AfterSend,
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

/// A request from `tester` to the functional group.
const fn functional_from(tester: Address) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: tester,
        ta: Address(0xE400),
        ta_type: TaType::Functional,
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

/// An application whose one service may be slow and admits a response-pending.
#[derive(Debug)]
struct Patient {
    /// How many times the next `read` pends before answering.
    pends: u8,
}

impl uds_services::ReadDataByIdentifier for Patient {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.pends)).await;
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}

uds_server! {
    Patient: ReadDataByIdentifier;
    transport = Script,
    peers = 1,
    server = PatientSrv,
}

/// Step until the transport errors, which it does only once the script is exhausted.
/// Bounded, so a driver that stopped consuming the script fails rather than hangs.
macro_rules! run {
    ($server:expr) => {{
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        for _ in 0..STEPS {
            let mut step = core::pin::pin!($server.step());
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
    }};
}

fn run(server: &mut SlowSrv) {
    run!(server);
}

const READ: &[u8] = &[0x22, 0xF4, 0x0D];
const POSITIVE: &[u8] = &[0x62, 0xF4, 0x0D, 0x40];

/// ``UDSSVC_ARCH_0032`` — a 0x78 is admissible only where `may_respond_pending` says so,
/// and the driver does not decide. A slow handler of a service declaring
/// `MAY_RESPOND_PENDING = false` overruns `tP2_Server` and gets no response-pending: the
/// tester sees only the final response.
///
/// Here rather than in `end_to_end.rs`: that fixture's only slow handler is its
/// `ReadDataByIdentifier`, whose 0x78 its other tests rely on, and none of its other
/// handlers pends.
#[test]
fn a_service_that_may_not_pend_gets_no_response_pending() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::At(50), // tP2_Server reached, mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok),
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
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));
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
            Step::Conf(response_to(TESTER), SResult::Ok), // frees the one association
            Step::Conf(response_to(OTHER_TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));
    assert_eq!(t.addressed(1), (Some(response_to(OTHER_TESTER)), POSITIVE));
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
            Step::Conf(response_to(TESTER), SResult::Ok),
            Step::Conf(response_to(OTHER_TESTER), SResult::Ok),
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
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));
    assert_eq!(t.addressed(1), (Some(response_to(OTHER_TESTER)), POSITIVE));
}

/// `7F 22 21` — `busyRepeatRequest` for a `ReadDataByIdentifier` request.
const BUSY: &[u8] = &[0x7F, 0x22, 0x21];
/// Another `ReadDataByIdentifier` request, distinguishable from [`READ`].
const READ_OTHER: &[u8] = &[0x22, 0xF1, 0x90];

/// ISO 14229-1:2020 8.7.6 — a second physically addressed request arriving while the
/// first is handled finds the protocol instance occupied and is answered
/// `busyRepeatRequest` (Annex A); the first is still answered, once the refusal's
/// confirmation frees the client's addressing (``UDSS_LLR_0061``).
#[test]
fn a_physical_request_mid_service_is_answered_busy() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok), // the refusal's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// Another client's link closing while the final response waits on the refusal's
/// confirmation ends nothing: the wait goes on, and the response is sent.
#[test]
fn another_clients_close_while_a_response_waits_ends_nothing() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler
            Step::Close(OTHER_TESTER),                   // while the response waits
            Step::Conf(response_to(TESTER), SResult::Ok), // the refusal's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 5, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// ``UDSS_LLR_0187`` end to end — the refusal leaves the service in progress its window:
/// `tP2_Server` still runs out on time after the refusal is confirmed, and the 0x78 it
/// owes goes out. Sent as a final response, the refusal would have stopped that timer and
/// no 0x78 would follow.
#[test]
fn a_busy_refusal_leaves_the_service_in_progress_its_response_pending() {
    let mut server = PatientSrv::new(
        Patient { pends: 3 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok), // the refusal's
            Step::At(50),                                // tP2_Server, mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 6, "the script was not consumed");
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(
        t.addressed(1),
        (Some(response_to(TESTER)), &[0x7F, 0x22, 0x78][..])
    );
    assert_eq!(t.addressed(2), (Some(response_to(TESTER)), POSITIVE));
}

/// A 0x78 that comes due while a busy refusal to the same client is unconfirmed is
/// refused for the association (``UDSS_LLR_0061``), and the overrun is reported once; it
/// is resubmitted on the confirmation that frees the association, not lost.
#[test]
fn a_response_pending_held_up_by_a_busy_refusal_still_goes_out() {
    let mut server = PatientSrv::new(
        Patient { pends: 3 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler
            Step::At(50), // tP2_Server, the refusal unconfirmed
            Step::Conf(response_to(TESTER), SResult::Ok), // the refusal's
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 6, "the script was not consumed");
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(
        t.addressed(1),
        (Some(response_to(TESTER)), &[0x7F, 0x22, 0x78][..])
    );
    assert_eq!(t.addressed(2), (Some(response_to(TESTER)), POSITIVE));
}

/// ISO 14229-1:2020 8.7.6 — occupancy holds "regardless of addressing mode": a
/// functionally addressed request mid-service is refused too, and the refusal goes to the
/// client physically. 8.7.5 suppresses no `busyRepeatRequest`.
#[test]
fn a_functional_request_mid_service_is_answered_busy() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(functional_from(TESTER), READ_OTHER), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok),
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// ISO 14229-1:2020 8.7.6's first exception — the functionally addressed `3E 80`
/// bypasses the service in progress: nothing answers it, and the service's own response
/// goes out unhindered. The same bytes physically addressed, or a functional
/// `TesterPresent` not suppressing its response, are ordinary occupancy.
#[test]
fn a_functional_keep_alive_mid_service_bypasses_it() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(functional_from(TESTER), &[0x3E, 0x80]), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 3, "the script was not consumed");
    assert_eq!(t.sent_count, 1);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));

    for (ai, tester_present) in [
        (request_from(TESTER), &[0x3E, 0x80][..]),
        (functional_from(TESTER), &[0x3E, 0x00][..]),
    ] {
        let mut server = SlowSrv::new(
            Slow { pends: 1 },
            Script::new(&[
                Step::Ind(request_from(TESTER), READ),
                Step::Ind(ai, tester_present), // mid-handler
                Step::Conf(response_to(TESTER), SResult::Ok),
                Step::Conf(response_to(TESTER), SResult::Ok),
            ]),
            ECU,
            PARAMS,
        );
        run(&mut server);
        let t = server.transport();
        assert_eq!(t.sent_count, 2, "{ai:?} {tester_present:02X?}");
        assert_eq!(
            t.addressed(0),
            (Some(response_to(TESTER)), &[0x7F, 0x3E, 0x21][..])
        );
    }
}

/// ISO 14229-1:2020 8.7.6 — a message too long for the concurrent buffer is occupancy
/// like any other, refused from the service identifier that fit; its length is the
/// buffer's limit, not this server's, so it is never `0x13`.
#[test]
fn a_message_too_long_for_the_concurrent_buffer_is_answered_busy() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::TooLong(
                request_from(TESTER),
                &[0x2E, 0xF1, 0x90, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            ), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok),
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(
        t.addressed(0),
        (Some(response_to(TESTER)), &[0x7F, 0x2E, 0x21][..])
    );
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// Another client's link closing while a handler runs ends nothing: the request in
/// progress is answered.
#[test]
fn another_clients_close_mid_handler_leaves_the_request_in_progress() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Close(OTHER_TESTER), // mid-handler
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 3, "the script was not consumed");
    assert_eq!(t.sent_count, 1);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));
}

/// A link closed while a handler runs ends the request with nothing sent, the abandoned
/// handler's bytes included, and the server goes on to answer the next request.
#[test]
fn a_close_mid_handler_sends_nothing_and_the_server_carries_on() {
    let mut server = SlowSrv::new(
        Slow { pends: 1 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Close(TESTER), // mid-handler
            Step::Ind(request_from(TESTER), READ),
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 1);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), POSITIVE));
}

/// An application whose sessions time differently: the default session as [`PARAMS`]
/// does, the extended one with a wider `P2` and a narrower `P2*`.
#[derive(Debug)]
struct Timed {
    /// How many times the next `read` pends before answering.
    pends: u8,
}

impl uds_services::ReadDataByIdentifier for Timed {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.pends)).await;
        out.write_all(&[0x40]).map_err(|_| Nrc::ResponseTooLong)
    }
}

impl uds_services::DiagnosticSessionControl for Timed {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
    }
    /// The extended session is entered from the default one only.
    fn supported_from(&self, s: S, active: S) -> bool {
        !(s == S::ExtendedDiagnosticSession && active == S::ExtendedDiagnosticSession)
    }
    fn leaves_running_software(&self, _s: S) -> bool {
        false
    }
    fn timing(&self, s: S) -> SessionTiming {
        match s {
            S::ExtendedDiagnosticSession => SessionTiming {
                p2_server_max_ms: 100,
                p2_star_server_max_10ms: 200,
            },
            _ => SessionTiming {
                p2_server_max_ms: 50,
                p2_star_server_max_10ms: 500,
            },
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _entered: S, _r: bool) {}
}

uds_server! {
    Timed: ReadDataByIdentifier, DiagnosticSessionControl;
    transport = Script,
    peers = 1,
    server = TimedSrv,
}

/// The pair a `DiagnosticSessionControl` response advertises is the pair enforced once
/// its session is in force: the next request's `tP2_Server` runs the extended session's
/// 100 ms, not [`PARAMS`]' 50 ms, and its enhanced window that session's 2000 ms, not
/// 5000 ms. Each 0x78 goes out when its own session's window comes due.
#[test]
fn the_confirmed_sessions_advertised_timing_is_enforced() {
    let mut server = TimedSrv::new(
        Timed { pends: 3 },
        Script::new(&[
            Step::Ind(request_from(TESTER), &[0x10, 0x03]),
            Step::Conf(response_to(TESTER), SResult::Ok), // the extended session takes effect
            Step::Ind(request_from(TESTER), READ),
            Step::At(50),  // PARAMS' tP2_Server would end here, mid-handler
            Step::At(100), // the extended session's does
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's: tP2*_Server starts
            Step::At(2_100), // the extended session's tP2*_Server ends
            Step::Conf(response_to(TESTER), SResult::Ok), // the second 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 9, "the script was not consumed");
    assert_eq!(t.sent_count, 4);
    // Advertised: P2 100 ms, P2* 200 x 10 ms.
    assert_eq!(t.addressed(0).1, &[0x50, 0x03, 0x00, 0x64, 0x00, 0xC8]);
    let pending = &[0x7F, 0x22, 0x78][..];
    assert_eq!(
        (t.addressed(1).1, t.sent(1).map_or(0, |s| s.at)),
        (pending, 100)
    );
    assert_eq!(
        (t.addressed(2).1, t.sent(2).map_or(0, |s| s.at)),
        (pending, 2_100)
    );
    assert_eq!(t.addressed(3).1, POSITIVE);
}

/// A request longer than the in-flight buffer, with no service in progress, is checked as
/// clause 8.7 checks any request, in ISO 14229-1:2020 Figure 5's and Figure 6's order:
/// `serviceNotSupported` (0x11) for a service this server lacks, `subFunctionNotSupported`
/// (0x12) for a sub-function it lacks, and only then
/// `incorrectMessageLengthOrInvalidFormat` (0x13), that buffer holding the longest request
/// any assembled service accepts. Never `busyRepeatRequest`, which would have the client
/// repeat it forever. 0x11 and 0x12 to a functional request are suppressed (8.7.5). The
/// session checks precede 0x13 too: 0x7F is pinned in `composition.rs`, and 0x7E in
/// [`an_over_long_request_for_a_sub_function_not_in_this_session_is_refused_0x7e`].
///
/// `Timed`'s in-flight buffer is six bytes, `DiagnosticSessionControl`'s bound, so what
/// fits of `SESSION` is itself too long and 0x13 comes from decoding it;
/// [`an_over_long_request_whose_front_decodes_is_not_handled`] pins the 0x13 a request
/// whose front decodes owes.
#[test]
fn a_request_too_long_for_any_service_is_refused_in_figure_5s_order() {
    const NOT_SUPPORTED: &[u8] = &[0x2E, 0xF1, 0x90, 1, 2, 3, 4, 5, 6];
    const NO_SUCH_SESSION: &[u8] = &[0x10, 0x05, 0, 0, 0, 0, 0];
    const SESSION: &[u8] = &[0x10, 0x03, 0, 0, 0, 0, 0];
    for (ai, request, expected) in [
        (request_from(TESTER), SESSION, &[0x7F, 0x10, 0x13][..]),
        (request_from(TESTER), NOT_SUPPORTED, &[0x7F, 0x2E, 0x11][..]),
        (functional_from(TESTER), NOT_SUPPORTED, &[][..]),
        (
            request_from(TESTER),
            NO_SUCH_SESSION,
            &[0x7F, 0x10, 0x12][..],
        ),
        (functional_from(TESTER), NO_SUCH_SESSION, &[][..]),
    ] {
        let mut server = TimedSrv::new(
            Timed { pends: 0 },
            Script::new(&[
                Step::TooLong(ai, request),
                Step::Conf(response_to(TESTER), SResult::Ok),
            ]),
            ECU,
            PARAMS,
        );
        run!(&mut server);
        let t = server.transport();
        assert_eq!(
            t.sent_count,
            usize::from(!expected.is_empty()),
            "{request:02X?}"
        );
        if !expected.is_empty() {
            assert_eq!(
                t.addressed(0),
                (Some(response_to(TESTER)), expected),
                "{request:02X?}"
            );
        }
    }
}

/// What fit of an over-long request can itself be a whole request — `22 F4 0D`, `Slow`'s
/// in-flight buffer being three bytes — and is still refused 0x13, never handled.
#[test]
fn an_over_long_request_whose_front_decodes_is_not_handled() {
    let mut server = SlowSrv::new(
        Slow { pends: 0 },
        Script::new(&[
            Step::TooLong(request_from(TESTER), &[0x22, 0xF4, 0x0D, 0xF4, 0x0E]),
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run(&mut server);
    let t = server.transport();
    assert_eq!(t.sent_count, 1);
    assert_eq!(
        t.addressed(0),
        (Some(response_to(TESTER)), &[0x7F, 0x22, 0x13][..])
    );
}

/// Figure 6's `subFunctionNotSupportedInActiveSession` (0x7E) precedes 0x13 for a request
/// longer than the in-flight buffer too: `Timed` enters the extended session from the
/// default one only.
#[test]
fn an_over_long_request_for_a_sub_function_not_in_this_session_is_refused_0x7e() {
    let mut server = TimedSrv::new(
        Timed { pends: 0 },
        Script::new(&[
            Step::Ind(request_from(TESTER), &[0x10, 0x03]),
            Step::Conf(response_to(TESTER), SResult::Ok), // the extended session takes effect
            Step::TooLong(request_from(TESTER), &[0x10, 0x03, 0, 0, 0, 0, 0]),
            Step::Conf(response_to(TESTER), SResult::Ok),
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 4, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(
        t.addressed(1),
        (Some(response_to(TESTER)), &[0x7F, 0x10, 0x7E][..])
    );
}

/// `7F 22 78` — the response-pending for a `ReadDataByIdentifier` request.
const PENDING: &[u8] = &[0x7F, 0x22, 0x78];

/// A request arriving mid-service at `tP2_Server` meets the overrun first: the 0x78 goes
/// out on time, and the busy refusal then finds the client's addressing awaiting that
/// 0x78's confirmation and is dropped (Annex J Figure J.2's other branch). Arriving in the
/// refusal's own input, the overrun would be reported there once and never answered.
#[test]
fn a_request_arriving_at_the_response_deadline_does_not_cost_the_response_pending() {
    let mut server = PatientSrv::new(
        Patient { pends: 2 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Slip(50), // tP2_Server, unreported
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler, at it
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 5, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(
        (t.addressed(0), t.sent(0).map_or(0, |s| s.at)),
        ((Some(response_to(TESTER)), PENDING), 50)
    );
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// The keep-alive `3E 80` arriving at `tP2_Server` does not cost the 0x78 either.
#[test]
fn a_keep_alive_arriving_at_the_response_deadline_does_not_cost_the_response_pending() {
    let mut server = PatientSrv::new(
        Patient { pends: 2 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Slip(50),
            Step::Ind(functional_from(TESTER), &[0x3E, 0x80]),
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 5, "the script was not consumed");
    assert_eq!(t.sent_count, 2);
    assert_eq!(
        (t.addressed(0), t.sent(0).map_or(0, |s| s.at)),
        ((Some(response_to(TESTER)), PENDING), 50)
    );
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}

/// A confirmation arriving at `tP2_Server` — here the busy refusal's — does not cost the
/// 0x78: refused while the refusal holds the client's addressing, it is owed, and sent
/// once this confirmation has freed it.
#[test]
fn a_confirmation_arriving_at_the_response_deadline_does_not_cost_the_response_pending() {
    let mut server = PatientSrv::new(
        Patient { pends: 3 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Ind(request_from(TESTER), READ_OTHER), // mid-handler
            Step::Slip(50),
            Step::Conf(response_to(TESTER), SResult::Ok), // the refusal's, at tP2_Server
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.cursor, 6, "the script was not consumed");
    assert_eq!(t.sent_count, 3);
    assert_eq!(t.addressed(0), (Some(response_to(TESTER)), BUSY));
    assert_eq!(
        (t.addressed(1), t.sent(1).map_or(0, |s| s.at)),
        ((Some(response_to(TESTER)), PENDING), 50)
    );
    assert_eq!(t.addressed(2), (Some(response_to(TESTER)), POSITIVE));
}

/// The keep-alive is indicated as keep-alive, not as a request: one would replace the
/// service in progress (``UDSS_LLR_0108``) and restart its `tP2_Server` from the
/// keep-alive's arrival, moving the 0x78 from 50 to 80. With one tester, the controlling
/// client's own request has stopped `tS3_Server`, so ``UDSS_LLR_0096`` has the keep-alive
/// change nothing at all.
#[test]
fn a_keep_alive_mid_service_leaves_the_response_window_where_it_was() {
    let mut server = PatientSrv::new(
        Patient { pends: 2 },
        Script::new(&[
            Step::Ind(request_from(TESTER), READ),
            Step::Slip(30),
            Step::Ind(functional_from(TESTER), &[0x3E, 0x80]), // mid-handler
            Step::At(50),
            Step::At(80),
            Step::Conf(response_to(TESTER), SResult::Ok), // the 0x78's
            Step::Conf(response_to(TESTER), SResult::Ok), // the final response's
        ]),
        ECU,
        PARAMS,
    );
    run!(&mut server);
    let t = server.transport();
    assert_eq!(t.sent_count, 2);
    assert_eq!(
        (t.addressed(0), t.sent(0).map_or(0, |s| s.at)),
        ((Some(response_to(TESTER)), PENDING), 50)
    );
    assert_eq!(t.addressed(1), (Some(response_to(TESTER)), POSITIVE));
}
