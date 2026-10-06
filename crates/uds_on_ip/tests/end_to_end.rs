//! A `uds_server!` server end to end over `DoIpTransport` and a socket-free
//! `DiagnosticEntity`, on `embassy-time`'s mock clock.

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
use support::{MockEntity, TESTER, Tester, Wire, block_on, exclusive_clock, now};
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, DataIdentifier, DiagnosticSessionControl, DiagnosticSessionType as S,
    ReadDataByIdentifier, RecordError, ResponseSink, ServerParams, SessionTiming,
    SessionTransition, Sink, TesterPresent, uds_server,
};

const ECU: Address = Address(0x0001);
const CONNECTION: ConnectionId = ConnectionId::new(0);

type Transport = DoIpTransport<MockEntity<1>>;

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

#[derive(Debug, Default)]
struct Ecu {
    transitions: Vec<SessionTransition>,
    /// How many times the next `read` pends before answering.
    slow: u8,
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
    fn leaves_running_software(&self, _s: S) -> bool {
        false
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, t: SessionTransition, _entered: S, _relocked: bool) {
        self.transitions.push(t);
    }
}
impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

uds_server! {
    Ecu: ReadDataByIdentifier, DiagnosticSessionControl, TesterPresent;
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
    EcuServer::new(
        Ecu::default(),
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
    let _clock = exclusive_clock();
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

/// A request longer than the server's receive buffer reaches the driver as
/// `DataTooLong`, never as a request made of its first bytes, and is refused
/// `incorrectMessageLengthOrInvalidFormat`.
#[test]
fn a_request_longer_than_the_server_accepts_is_refused_not_served_as_a_fragment() {
    let _clock = exclusive_clock();
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

/// ISO 14229-5:2022 REQ 7.9: `10 03` is answered `50 03`, the connection is
/// closed after that response is confirmed sent, and the session change executes
/// on the confirmation. `tS3_Server` then runs on the transport's clock and
/// returns the server to the default session (`UDSS_LLR_0100`).
#[test]
fn a_session_change_is_answered_then_the_connection_closed_then_it_times_out() {
    let _clock = exclusive_clock();
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x03]),
    ]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
    assert_eq!(
        s.services().transitions,
        [
            SessionTransition::DefaultToNonDefault,
            SessionTransition::NonDefaultToDefault,
        ]
    );
    assert_eq!(
        now().0,
        5_000,
        "tS3_Server expired on the clock it was set by"
    );
}

/// REQ 7.8 and REQ 7.10 put reconnection on the tester; the session is the
/// server's, not the connection's, so the tester returning on a new connection
/// finds the extended session still active: programming, entered only from
/// extended, is accepted, and closes the connection in turn.
#[test]
fn a_tester_returning_after_the_close_finds_its_session() {
    let _clock = exclusive_clock();
    let mut s = server([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x03]),
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x02]),
    ]);
    run(&mut s);
    assert_eq!(
        wire(&s),
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
            Wire::Data(CONNECTION, vec![0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
}

/// A tester that leaves while its request is being served ends the exchange: the
/// close is unexpected, the handler is abandoned, and nothing is sent to a
/// connection that is gone.
#[test]
fn a_tester_leaving_mid_service_abandons_the_exchange() {
    let _clock = exclusive_clock();
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
