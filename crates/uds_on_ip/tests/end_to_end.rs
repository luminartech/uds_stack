//! A `uds_server!` server end to end over `DoIpTransport` and a socket-free
//! `DiagnosticEntity`, on the mock entity's clock.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test harness: scripts are fixed and futures complete in bounded polls"
)]

#[allow(dead_code, reason = "each test binary uses part of the shared mock")]
mod support;

use core::future::Future;
use simple_doip::service::ConnectionId;
use support::{MockEntity, TESTER, Tester, Wire, block_on};
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Access, Address, DataIdentifier, DiagnosticSessionControl, DiagnosticSessionType as S,
    EcuReset, ReadDataByIdentifier, RecordError, ResetType, ResponseSink, ServerParams,
    SessionTiming, SessionTransition, Sessions, Sink, TesterPresent, UdsTransport,
    uds_server,
};

const ECU: Address = Address(0x0001);
const CONNECTION: ConnectionId = ConnectionId::new(0);

type Transport = DoIpTransport<MockEntity<1>, 1>;

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

/// The software a server runs, which decides whether a session change leaves it
/// (ISO 14229-1:2020 10.2.2.2 Table 25).
#[derive(Debug, Default, Clone, Copy)]
enum Software {
    /// Programs from within itself: no session change leaves it.
    #[default]
    ProgramsInPlace,
    /// Jumps to its bootloader for the programming session.
    Application,
    /// Restarts the application for the default session.
    Bootloader,
}

#[derive(Debug, Default)]
struct Ecu {
    runs: Software,
    transitions: Vec<SessionTransition>,
    entered: Vec<S>,
    /// How many times the next `read` pends before answering.
    slow: u8,
    /// How many times the next `reset` pends before accepting.
    reset_slow: u8,
}
impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = true;
    const MAX_DIDS_PER_REQUEST: usize = 2;
    async fn read(&mut self, _did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.slow)).await;
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
    /// Programming is entered only from Extended; every other session from any.
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    fn leaves_running_software(&self, s: S) -> bool {
        match self.runs {
            Software::ProgramsInPlace => false,
            Software::Application => matches!(s, S::ProgrammingSession),
            Software::Bootloader => matches!(s, S::DefaultSession),
        }
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, t: SessionTransition, entered: S, _relocked: bool) {
        self.transitions.push(t);
        self.entered.push(entered);
    }
}
impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = true;
    fn access(&self, _kind: ResetType) -> Option<Access> {
        Some(Access::new(Sessions::ALL))
    }
    async fn reset(
        &mut self,
        _kind: ResetType,
        _out: &mut ResponseSink<'_>,
    ) -> Result<(), Nrc> {
        PendN(core::mem::take(&mut self.reset_slow)).await;
        Ok(())
    }
}
impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

uds_server! {
    Ecu: ReadDataByIdentifier, DiagnosticSessionControl, EcuReset, TesterPresent;
    transport = Transport,
    peers = 1,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

fn server(script: impl IntoIterator<Item = Tester>) -> EcuServer {
    server_running(Software::default(), script)
}

fn server_running(runs: Software, script: impl IntoIterator<Item = Tester>) -> EcuServer {
    EcuServer::new(
        Ecu {
            runs,
            ..Ecu::default()
        },
        DoIpTransport::new(MockEntity::new(script), bench_reloads()),
        ECU,
        PARAMS,
    )
}

/// Step until the entity has nothing left to do, and check the script was spent.
fn run(server: &mut EcuServer) {
    while block_on(server.step()).is_ok() {}
    assert!(
        server.transport().entity().script.is_empty(),
        "the run ended with tester actions unconsumed"
    );
}

fn wire(server: &EcuServer) -> &[Wire] {
    &server.transport().entity().wire
}

/// A physical request is indicated, answered on the tester's connection, and
/// confirmed (ISO 14229-5:2022 REQ 4.3 Table 4).
#[test]
fn a_physical_read_is_answered() {
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x22, 0xF4, 0x0D]),
    ]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [Wire::Data(CONNECTION, vec![0x62, 0xF4, 0x0D, 0x40])]
    );
}

/// A response the entity refuses is confirmed failed (ISO 13400-2:2019 8.3.1), and
/// the server carries on: the next request is answered.
#[test]
fn a_refused_response_does_not_stop_the_server() {
    let mut entity = MockEntity::new([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x22, 0xF4, 0x0D]),
        Tester::Sends(TESTER, vec![0x22, 0xF4, 0x0D]),
    ]);
    entity.refuse.push_back(true);
    let mut s = EcuServer::new(
        Ecu::default(),
        DoIpTransport::new(entity, bench_reloads()),
        ECU,
        PARAMS,
    );
    run(&mut s);
    assert_eq!(
        wire(&s),
        [Wire::Data(CONNECTION, vec![0x62, 0xF4, 0x0D, 0x40])]
    );
}

/// A request longer than the server's receive buffer reaches the driver as
/// `DataTooLong`, never as a request made of its first bytes, and is refused
/// `incorrectMessageLengthOrInvalidFormat`.
#[test]
fn a_request_longer_than_the_server_accepts_is_refused_not_served_as_a_fragment() {
    let mut long = vec![0x22];
    long.extend([0xF4, 0x0D].repeat(100));
    let mut s = server([Tester::Connects(TESTER), Tester::Sends(TESTER, long)]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [Wire::Data(
            CONNECTION,
            vec![
                0x7F,
                0x22,
                Nrc::IncorrectMessageLengthOrInvalidFormat.into()
            ]
        )]
    );
}

/// `10 03` is answered, the session change executes on the response's
/// confirmation, and the connection stays: ISO 14229-5:2022 REQ 7.9 closes it only
/// where the change disconnects, which this server has not said. `tS3_Server` then
/// runs on the transport's clock and returns the server to the default session
/// (`UDSS_LLR_0100`).
#[test]
fn a_session_change_keeps_the_connection_and_times_out_on_the_transports_clock() {
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x03]),
    ]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [Wire::Data(
            CONNECTION,
            vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]
        )]
    );
    assert_eq!(
        s.services().transitions,
        [
            SessionTransition::DefaultToNonDefault,
            SessionTransition::NonDefaultToDefault,
        ]
    );
    assert_eq!(
        s.transport().now().0,
        5_000,
        "tS3_Server expired on the clock it was set by"
    );
}

/// The tester stays on its connection through a session change: programming,
/// entered only from extended, is accepted on the same connection that selected
/// extended.
#[test]
fn a_tester_stays_connected_through_a_session_change() {
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x03]),
        Tester::Sends(TESTER, vec![0x10, 0x02]),
    ]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Data(CONNECTION, vec![0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
        ]
    );
}

/// ISO 14229-5:2022 REQ 7.11 through the driver: a reset that takes long enough to
/// be answered response-pending keeps the connection through each `7F 11 78`, and
/// the final `51 01` is sent, then the connection is closed.
#[test]
fn a_reset_answered_pending_closes_the_connection_after_51_01() {
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x11, 0x01]),
    ]);
    s.services().reset_slow = 200;
    run(&mut s);
    let wire = wire(&s);
    let (pending, last_two) = wire.split_last_chunk::<2>().unwrap();
    assert!(!pending.is_empty(), "the reset was answered pending first");
    assert!(
        pending
            .iter()
            .all(|sent| *sent == Wire::Data(CONNECTION, vec![0x7F, 0x11, 0x78])),
        "{pending:?}"
    );
    assert_eq!(
        last_two,
        &[
            Wire::Data(CONNECTION, vec![0x51, 0x01]),
            Wire::Close(CONNECTION),
        ]
    );
}

/// An application entering its bootloader: `50 02` is sent, then the connection is
/// closed (ISO 14229-5:2022 REQ 7.9, Figure 5), and the application learns which
/// session it entered so it can jump. `50 03` on the way closes nothing.
#[test]
fn an_application_entering_its_bootloader_closes_the_connection_after_50_02() {
    let mut s = server_running(
        Software::Application,
        [
            Tester::Connects(TESTER),
            Tester::Sends(TESTER, vec![0x10, 0x03]),
            Tester::Sends(TESTER, vec![0x10, 0x02]),
        ],
    );
    run(&mut s);
    assert_eq!(
        wire(&s),
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Data(CONNECTION, vec![0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
    assert_eq!(
        s.services().entered[..2],
        [S::ExtendedDiagnosticSession, S::ProgrammingSession]
    );
}

/// A bootloader returning to the application: entering programming keeps the
/// connection, and `50 01` is sent, then the connection is closed.
#[test]
fn a_bootloader_returning_to_the_application_closes_the_connection_after_50_01() {
    let mut s = server_running(
        Software::Bootloader,
        [
            Tester::Connects(TESTER),
            Tester::Sends(TESTER, vec![0x10, 0x03]),
            Tester::Sends(TESTER, vec![0x10, 0x02]),
            Tester::Sends(TESTER, vec![0x10, 0x01]),
        ],
    );
    run(&mut s);
    assert_eq!(
        wire(&s),
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Data(CONNECTION, vec![0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Data(CONNECTION, vec![0x50, 0x01, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
    assert_eq!(s.services().entered.last(), Some(&S::DefaultSession));
}

/// A tester that leaves while its request is being served ends the exchange: the
/// close is unexpected, the handler is abandoned, and nothing is sent to a
/// connection that is gone.
#[test]
fn a_tester_leaving_mid_service_abandons_the_exchange() {
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x22, 0xF4, 0x0D]),
        Tester::Leaves(TESTER),
    ]);
    s.services().slow = 3;
    run(&mut s);
    assert_eq!(
        s.transport().entity().requested,
        Vec::<Vec<u8>>::new(),
        "the handler was abandoned before it answered"
    );
}
