//! ISO 14229-1:2020 clause 10.3 — an accepted `ECUReset` runs once its response is
//! confirmed sent (``UDSSVC_ARCH_0045``), through `Server::step` over a scripted transport.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test harness: scripts are fixed arrays and futures complete in bounded polls"
)]

mod common;

use common::{Script, Step, block_on};
use core::future::{Future, ready};
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Access, Address, AfterReset, Ai, DiagnosticSessionControl, DiagnosticSessionType as S,
    EcuReset, Mtype, ResetType, SResult, ServerParams, SessionTiming, SessionTransition,
    Sessions, Stepped, Stopped, TaType, TesterPresent, uds_server,
};
use uds_session::TransportError;

const TESTER: Address = Address(0x0E80);
const ECU: Address = Address(0x0010);
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
fn script(steps: &[Step]) -> Script {
    Script::new(steps).sending_only(|ai| ai == to_tester())
}
fn ind(bytes: &'static [u8]) -> Step {
    Step::Ind(from_tester(TaType::Physical), bytes)
}
fn conf(result: SResult) -> Step {
    Step::Conf(to_tester(), result)
}

#[derive(Debug, Default)]
struct Ecu {
    transitions: Vec<SessionTransition>,
    accepted: Vec<ResetType>,
    confirmed: Vec<ResetType>,
}

impl DiagnosticSessionControl for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
    }
    /// Programming is entered only from Extended or Programming, so `10 02` is 0x7E from
    /// Default and accepted in Programming: what tells a test which session it is in.
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession | S::ProgrammingSession)
    }
    fn leaves_running_software(&self, _s: S) -> bool {
        false
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max: uds_services::P2StarServerMax::from_tens_of_millis(500),
        }
    }
    fn prepare(&mut self, _s: S) -> impl core::future::Future<Output = Result<(), Nrc>> {
        core::future::ready(Ok(()))
    }
    fn on_transition(&mut self, t: SessionTransition, _entered: S, _relocked: bool) {
        self.transitions.push(t);
    }
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, kind: ResetType) -> Option<Access> {
        matches!(
            kind,
            ResetType::HardReset
                | ResetType::SoftReset
                | ResetType::EnableRapidPowerShutDown
        )
        .then(|| Access::new(Sessions::ALL))
    }
    fn reset(&mut self, kind: ResetType) -> impl Future<Output = Result<AfterReset, Nrc>> {
        self.accepted.push(kind);
        ready(Ok(if matches!(kind, ResetType::EnableRapidPowerShutDown) {
            AfterReset::Continue
        } else {
            AfterReset::Restart
        }))
    }
    fn power_down_time(&self) -> u8 {
        0x0A
    }
    fn confirmed(&mut self, kind: ResetType) {
        self.confirmed.push(kind);
    }
}

uds_server! {
    Ecu: DiagnosticSessionControl, TesterPresent, EcuReset;
    transport = Script,
    peers = 1,
    services = EcuServices,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 0);

/// The steps that enter the programming session (`10 03`, then `10 02`, each confirmed),
/// then `after`.
fn steps(after: &[Step]) -> Vec<Step> {
    let mut all = vec![
        ind(&[0x10, 0x03]),
        conf(SResult::Ok),
        ind(&[0x10, 0x02]),
        conf(SResult::Ok),
    ];
    all.extend_from_slice(after);
    all
}

fn step(s: &mut EcuServer) -> Option<Stopped> {
    match block_on(s.step()).unwrap() {
        Stepped::Continue => None,
        Stepped::Stopped(stopped) => Some(stopped),
    }
}

/// 10.3, Annex I transition 1 — a restarting reset changes nothing until its response is
/// confirmed; then `confirmed` and `on_transition` (Figure 7 key 4) are called, the server
/// is in the default session (`10 02` is 0x7E again), and `step` reports `Stopped`.
#[test]
fn a_restarting_reset_runs_on_its_confirmation_and_stops() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[
            ind(&[0x11, 0x01]),
            conf(SResult::Ok),
            ind(&[0x10, 0x02]),
            conf(SResult::Ok),
        ])),
        ECU,
        PARAMS,
    );
    for _ in 0..4 {
        assert_eq!(step(&mut s), None);
    }
    assert_eq!(step(&mut s), None, "the reset's request");
    assert_eq!(s.services().confirmed, []);
    assert_eq!(s.services().transitions.len(), 2);
    assert_eq!(step(&mut s), Some(Stopped::Reset(ResetType::HardReset)));
    assert_eq!(s.services().confirmed, [ResetType::HardReset]);
    assert_eq!(
        s.services().transitions.last(),
        Some(&SessionTransition::NonDefaultToDefault)
    );
    assert_eq!(step(&mut s), None);
    assert_eq!(s.transport().sent_bytes(2), &[0x51, 0x01]);
    assert_eq!(s.transport().sent_bytes(3), &[0x7F, 0x10, 0x7E]);
}

/// ISO 14229-5:2022 REQ 7.11 — a restarting reset tells the transport it is restarting
/// as `step` reports it, whether its response was sent or suppressed, so a transport can
/// close what the restart ends. A reset that does not restart tells it nothing.
#[test]
fn a_restart_tells_the_transport_and_a_continuing_reset_does_not() {
    for (request, restarts) in [
        (&[0x11, 0x01][..], 1),
        (&[0x11, 0x81][..], 1),
        (&[0x11, 0x04][..], 0),
    ] {
        let mut s = EcuServer::new(
            Ecu::default(),
            script(&[ind(request), conf(SResult::Ok)]),
            ECU,
            PARAMS,
        );
        for _ in 0..2 {
            step(&mut s);
        }
        assert_eq!(s.transport().restarts, restarts, "{request:02X?}");
    }
}

/// A restart from the default session changes no session, so no session hook is called.
#[test]
fn a_restart_from_the_default_session_calls_no_session_hook() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&[ind(&[0x11, 0x01]), conf(SResult::Ok)]),
        ECU,
        PARAMS,
    );
    assert_eq!(step(&mut s), None, "the reset's request");
    assert_eq!(step(&mut s), Some(Stopped::Reset(ResetType::HardReset)));
    assert_eq!(s.services().confirmed, [ResetType::HardReset]);
    assert_eq!(s.services().transitions, []);
}

/// ``UDSS_LLR_0086`` — a suppressed `11 81` runs on its completion report, physically and
/// functionally addressed alike.
#[test]
fn a_suppressed_reset_runs_on_its_completion_report() {
    for ta_type in [TaType::Physical, TaType::Functional] {
        let mut s = EcuServer::new(
            Ecu::default(),
            script(&steps(&[Step::Ind(from_tester(ta_type), &[0x11, 0x81])])),
            ECU,
            PARAMS,
        );
        for _ in 0..4 {
            assert_eq!(step(&mut s), None);
        }
        assert_eq!(
            step(&mut s),
            Some(Stopped::Reset(ResetType::HardReset)),
            "{ta_type:?}"
        );
        assert_eq!(s.services().confirmed, [ResetType::HardReset]);
        assert_eq!(s.transport().sent_count, 2, "{ta_type:?}");
    }
}

/// Table 34 — a reset that does not restart is confirmed to the application and the
/// server keeps serving in its session (`10 02` from Programming is accepted).
#[test]
fn a_continuing_reset_is_confirmed_and_serving_continues() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[
            ind(&[0x11, 0x04]),
            conf(SResult::Ok),
            ind(&[0x10, 0x02]),
            conf(SResult::Ok),
        ])),
        ECU,
        PARAMS,
    );
    for _ in 0..8 {
        assert_eq!(step(&mut s), None);
    }
    assert_eq!(
        s.services().confirmed,
        [ResetType::EnableRapidPowerShutDown]
    );
    assert_eq!(s.transport().sent_bytes(2), &[0x51, 0x04, 0x0A]);
    assert_eq!(s.transport().sent_bytes(3).first(), Some(&0x50));
}

/// ``UDSS_LLR_0085`` acts on a successful transmission only: a reset whose confirmation
/// fails runs nothing, and the server serves on in its session.
#[test]
fn a_failed_reset_confirmation_runs_nothing() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[
            ind(&[0x11, 0x01]),
            conf(SResult::Transport(TransportError(1))),
            ind(&[0x10, 0x02]),
            conf(SResult::Ok),
        ])),
        ECU,
        PARAMS,
    );
    for _ in 0..8 {
        assert_eq!(step(&mut s), None);
    }
    assert_eq!(s.services().confirmed, []);
    assert_eq!(s.transport().sent_bytes(3).first(), Some(&0x50));
}

/// 10.3.1 — a request between the reset's response and its confirmation is not served.
#[test]
fn a_request_before_the_reset_is_confirmed_gets_no_response() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[
            ind(&[0x11, 0x01]),
            ind(&[0x10, 0x03]),
            conf(SResult::Ok),
        ])),
        ECU,
        PARAMS,
    );
    for _ in 0..6 {
        assert_eq!(step(&mut s), None);
    }
    assert_eq!(step(&mut s), Some(Stopped::Reset(ResetType::HardReset)));
    assert_eq!(
        s.transport().sent_count,
        3,
        "the 0x10 03 during the window is unanswered"
    );
}

/// Annex I transition 1 — after `Stopped`, the next step runs start-up again and serves
/// in the default session.
#[test]
fn after_a_restart_the_next_step_runs_start_up_again() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[
            ind(&[0x11, 0x01]),
            conf(SResult::Ok),
            ind(&[0x10, 0x03]),
            conf(SResult::Ok),
        ])),
        ECU,
        PARAMS,
    );
    while step(&mut s).is_none() {}
    assert_eq!(step(&mut s), None);
    assert_eq!(
        s.transport().sent_bytes(3),
        &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]
    );
}

/// `into_parts` gives the services back with what they recorded.
#[test]
fn into_parts_returns_the_services() {
    let mut s = EcuServer::new(
        Ecu::default(),
        script(&steps(&[ind(&[0x11, 0x01]), conf(SResult::Ok)])),
        ECU,
        PARAMS,
    );
    while step(&mut s).is_none() {}
    let (ecu, transport) = s.into_parts();
    assert_eq!(ecu.confirmed, [ResetType::HardReset]);
    assert!(transport.finished());
}
