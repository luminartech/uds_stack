//! The tester's path end to end: a `uds_client!` client over `uds_on_ip`'s
//! `DoIpClientTransport` over `simple_doip`'s `Tester`, against the sensor's
//! `uds_server!` server over `DoIpTransport` over its `Entity`, on loopback sockets.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test harness: a failed expectation is the test failing"
)]

use doip_loopback::{
    ENTITY, Loopback, SensorClientTransport, SensorEntity, on_loopback, serve_until,
    ticking,
};
use embassy_time::Duration;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Address, ClientError, ClientTiming, DataIdentifier, DiagnosticSessionControl,
    DiagnosticSessionType as S, KeepAlive, PhysicalKeepAlive, ReadDataByIdentifier,
    RecordError, Reloads, Response, ResponseSink, ServerParams, SessionTiming,
    SessionTransition, Sink, UdsTransport, uds_client, uds_server,
};

/// Two identifiers sharing a service and a high byte, so a positive response to one
/// echoes the same first data byte as the other's: the client's own service check cannot
/// tell them apart. Reading [`Did::Slow`] takes [`SLOW`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    Vin,
    Slow,
}

const VIN: &[u8] = b"WVWZZZ1JZXW000001";

/// How long reading [`Did::Slow`] takes: far past the client's response window.
const SLOW: Duration = Duration::from_millis(1_000);

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = VIN.len();
    fn as_u16(self) -> u16 {
        match self {
            Self::Vin => 0xF190,
            Self::Slow => 0xF18C,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xF190 => Some(Self::Vin),
            0xF18C => Some(Self::Slow),
            _ => None,
        }
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        let len = match self {
            Self::Vin => VIN.len(),
            Self::Slow => 1,
        };
        buf.split_at_checked(len).ok_or(RecordError::Short)
    }
}

/// An application that reads its identifiers and jumps to its bootloader for the
/// programming session, so leaving its running software on that change
/// (ISO 14229-1:2020 10.2.2.2 Table 25).
#[derive(Debug, Default)]
struct Ecu;

impl DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
    }
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    fn leaves_running_software(&self, s: S) -> bool {
        matches!(s, S::ProgrammingSession)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _entered: S, _relocked: bool) {}
}

impl ReadDataByIdentifier for Ecu {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 1;
    async fn read(&mut self, did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        match did {
            Did::Vin => out.write_all(VIN).map_err(|_| Nrc::ResponseTooLong),
            Did::Slow => {
                embassy_time::Timer::after(SLOW).await;
                out.write_all(&[0x5A]).map_err(|_| Nrc::ResponseTooLong)
            }
        }
    }
}

uds_server! {
    Ecu: DiagnosticSessionControl, ReadDataByIdentifier;
    transport = DoIpTransport<SensorEntity<4096>, 2>,
    peers = 1,
    server = EcuServer,
}

uds_client! {
    Did;
    transport = SensorClientTransport,
    max_dids_per_request = 1,
    physical = 1,
    functional = 1,
    responders = 1,
    keep_alive = PhysicalKeepAlive,
    client = Diagnoser,
}

const SERVER: Address = Address(ENTITY.0);
const TESTER: Address = Address(doip_loopback::TESTER.0);

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

/// The client's response window, well short of [`SLOW`].
const RELOADS: Reloads = Reloads {
    default_reload: 100,
    enhanced_reload: 5_000,
};

/// How far [`ticking`] moves the clock at a time.
const STEP: Duration = Duration::from_millis(20);

fn server(loopback: &Loopback) -> EcuServer {
    EcuServer::new(
        Ecu,
        DoIpTransport::new(loopback.entity(), bench_reloads()),
        SERVER,
        PARAMS,
    )
}

async fn client(loopback: &Loopback, backoff: Duration) -> Diagnoser {
    let transport = loopback
        .client_transport(backoff, RELOADS)
        .await
        .expect("activated");
    Diagnoser::new(
        transport,
        TESTER,
        KeepAlive::physical(2_000),
        ClientTiming::new(0, 0, 0),
    )
}

/// The record a positive read of `did` returned, or a panic naming what came instead.
fn record(
    read: Result<
        Response<uds_services::Records<'_, Did>>,
        ClientError<impl core::fmt::Debug>,
    >,
    did: Did,
) -> Vec<u8> {
    match read {
        Ok(Response::Positive(mut records)) => match records.next() {
            Some((read, record)) if read == did => record.to_vec(),
            other => panic!("expected {did:?}'s record, got {other:?}"),
        },
        other => panic!("expected a positive response, got {other:?}"),
    }
}

/// A client reads a data identifier from the sensor's server over real sockets
/// (ISO 14229-5:2022 REQ 4.3 Table 4, both sides), and ends its session.
#[test]
fn a_client_reads_a_did_over_loopback() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut c = client(&loopback, Duration::from_ticks(0)).await;
            assert_eq!(
                record(
                    c.read_data_by_identifier(SERVER, &[Did::Vin]).await,
                    Did::Vin
                ),
                VIN
            );
            c.close().await.expect("closed");
        })
        .await;
    });
}

/// ISO 14229-5:2022 REQ 7.8 and REQ 7.9: the server closes the connection after its
/// positive response to a session change that leaves its software, and the client's next
/// call goes on a new connection, routing activated again, without the application
/// retrying.
#[test]
fn a_client_survives_the_prescribed_close() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut c = client(&loopback, Duration::from_ticks(0)).await;
            for session in [S::ExtendedDiagnosticSession, S::ProgrammingSession] {
                let changed = c.diagnostic_session_control(SERVER, session).await;
                assert!(matches!(changed, Ok(Response::Positive(_))), "{changed:?}");
            }
            assert_eq!(
                record(
                    c.read_data_by_identifier(SERVER, &[Did::Vin]).await,
                    Did::Vin
                ),
                VIN
            );
            c.close().await.expect("closed");
        })
        .await;
    });
}

/// Issue #17 item 3 and the open question "Should a reset discard a message already
/// arriving?": a response that never arrives in its window, then good reads. Each request
/// after an unanswered one goes on a new connection, so the server's answer to the slow
/// read, sharing the service and first data byte of the reads that follow, reaches none
/// of them: each read gets its own record.
#[test]
fn no_late_reply_reaches_a_later_request() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut c = client(&loopback, Duration::from_ticks(0)).await;
            let slow = ticking(c.read_data_by_identifier(SERVER, &[Did::Slow]), STEP).await;
            assert!(matches!(slow, Err(ClientError::Timeout)), "{slow:?}");

            for _ in 0..2 {
                let read =
                    ticking(c.read_data_by_identifier(SERVER, &[Did::Vin]), STEP).await;
                assert_eq!(record(read, Did::Vin), VIN);
            }
            c.close().await.expect("closed");
        })
        .await;
    });
}

/// Issue #17 item 2: the tester's address is still held by the connection the lost
/// response was awaited on when the next read needs a new one. The tester gives that
/// connection up first, waits its back-off, then connects, so the entity, which serves
/// one tester, has the address free when the new activation arrives, and the read is
/// answered.
#[test]
fn a_reconnect_drops_backs_off_then_connects() {
    const BACKOFF: Duration = Duration::from_millis(500);
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut c = client(&loopback, BACKOFF).await;
            let slow = ticking(c.read_data_by_identifier(SERVER, &[Did::Slow]), STEP).await;
            assert!(matches!(slow, Err(ClientError::Timeout)), "{slow:?}");

            let before = c.transport().now();
            let read = ticking(c.read_data_by_identifier(SERVER, &[Did::Vin]), STEP).await;
            assert_eq!(record(read, Did::Vin), VIN);
            let waited = c.transport().now().0.wrapping_sub(before.0);
            assert!(
                u64::from(waited) >= BACKOFF.as_millis(),
                "connected {waited} ms after the drop"
            );
            c.close().await.expect("closed");
        })
        .await;
    });
}
