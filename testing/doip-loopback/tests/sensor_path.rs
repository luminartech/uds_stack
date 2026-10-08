//! The sensor's path end to end: `uds_server!` → `DoIpTransport` → `simple_doip`'s
//! `Entity`, with `simple_doip`'s `Tester` at the other end of a loopback socket.
//!
//! `uds_on_ip`'s `end_to_end.rs` runs the same server over a scripted entity; this runs
//! it over the one the sensor ships, so what the entity does on the wire is what is
//! tested.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test harness: a failed expectation is the test failing"
)]

use core::future::Future;

use doip_loopback::{
    ENTITY, FUNCTIONAL, Loopback, SensorEntity, Then, advance, answered, ask, exchange,
    next, on_loopback, send, serve_until,
};
use simple_doip::TaType;
use simple_doip::messages::RoutingActivationResponseCode;
use simple_doip::service::TesterConnection;
use simple_doip::service::{DiagnosticEntity, DoIpResult};
use simple_doip::tester::ConnectError;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Access, Address, DataIdentifier, DiagnosticSessionControl, DiagnosticSessionType as S,
    EcuReset, ReadDataByIdentifier, RecordError, ResetType, ResponseSink, ServerParams,
    ServiceSet, SessionTiming, SessionTransition, Sessions, Sink, Storage, TesterPresent,
    uds_server,
};

/// The sensor's message size.
const MAX_MESSAGE: usize = 4096;

/// The sensor's transport: the entity serves one tester, so its table is two.
type Transport = DoIpTransport<SensorEntity<MAX_MESSAGE>, 2>;

/// An application that jumps to its bootloader for the programming session, and so
/// leaves its running software on that change (ISO 14229-1:2020 10.2.2.2 Table 25).
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
    /// Programming is entered only from Extended; every other session from any.
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

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, _kind: ResetType) -> Option<Access> {
        Some(Access::new(Sessions::ALL))
    }
    fn reset(
        &mut self,
        _kind: ResetType,
        _out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        core::future::ready(Ok(()))
    }
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent;
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

fn server(loopback: &Loopback) -> EcuServer {
    EcuServer::new(
        Ecu,
        DoIpTransport::new(loopback.entity(), bench_reloads()),
        Address(ENTITY.0),
        PARAMS,
    )
}

/// A request is indicated, answered on the connection it arrived on, and the answer
/// confirmed; physically and functionally addressed alike (ISO 14229-5:2022 REQ 4.3
/// Table 4; ISO 13400-2:2019 8.3.3).
#[test]
fn a_request_is_answered() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut tester, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            assert_eq!(
                exchange(&mut tester, FUNCTIONAL, TaType::Functional, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// An acknowledgement and the response to its request leave the entity in one write.
/// Neither backend disables Nagle's algorithm, so a response in a second write waits for
/// the tester to acknowledge the first, which a delayed acknowledgement holds back about
/// 40 ms, most of the 50 ms `P2_server` (ISO 14229-2:2013 Table 4). Twenty round trips
/// take nothing like that.
#[test]
fn a_response_does_not_wait_on_the_tester_acknowledging_its_acknowledgement() {
    const ROUND_TRIPS: u32 = 20;
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let started = std::time::Instant::now();
            for _ in 0..ROUND_TRIPS {
                assert_eq!(
                    ask(&mut tester, &[0x3E, 0x00]).await,
                    answered(&[0x7E, 0x00])
                );
            }
            let each = started.elapsed() / ROUND_TRIPS;
            assert!(
                each < std::time::Duration::from_millis(10),
                "{each:?} a round trip"
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A positive `ECUReset` response is followed by the entity closing the connection
/// (ISO 14229-5:2022 REQ 7.11), and the tester then reconnects and activates routing
/// again (REQ 7.10), on which it is served as before.
#[test]
fn a_reset_is_answered_then_closed_and_the_tester_reconnects() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut tester, &[0x11, 0x01]).await,
                answered(&[0x51, 0x01])
            );
            assert_eq!(next(&mut tester).await, Then::Closed);

            tester.reconnect().await.expect("reconnected");
            assert_eq!(
                ask(&mut tester, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A positive `DiagnosticSessionControl` response to a session change that leaves the
/// running software is followed by the entity closing the connection (ISO 14229-5:2022
/// REQ 7.9); a change that does not leave it closes nothing. The tester reconnects
/// (REQ 7.8).
#[test]
fn a_session_change_that_leaves_the_software_is_answered_then_closed() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let extended = ask(&mut tester, &[0x10, 0x03]).await;
            assert!(
                matches!(&extended.then, Then::Response(_, pdu) if pdu.starts_with(&[0x50, 0x03])),
                "{extended:?}"
            );
            let programming = ask(&mut tester, &[0x10, 0x02]).await;
            assert!(
                matches!(&programming.then, Then::Response(_, pdu) if pdu.starts_with(&[0x50, 0x02])),
                "{programming:?}"
            );
            assert_eq!(next(&mut tester).await, Then::Closed);

            tester.reconnect().await.expect("reconnected");
            assert_eq!(ask(&mut tester, &[0x3E, 0x00]).await, answered(&[0x7E, 0x00]));
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A second connection activating routing for the tester's address has the first
/// alive-checked (ISO 13400-2:2019 REQ 3.DoIP-091), and waits while the first has not
/// answered. Once it answers, the second is refused `0x03` (REQ 3.DoIP-093, Table 49),
/// and the first is served as before.
#[test]
fn a_second_connection_from_the_tester_is_refused_while_the_first_answers() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut first = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );

            let mut second = core::pin::pin!(loopback.tester());
            // The first is not read, so its alive check goes unanswered, and the clock
            // stands still, so the check does not time out: the second must wait.
            tokio::select! {
                biased;
                decided = &mut second => panic!("decided before the first answered: {decided:?}"),
                () = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
            let second = tokio::select! {
                second = second => second,
                then = next(&mut first) => panic!("the first saw {then:?}"),
            };
            assert!(
                matches!(
                    second,
                    Err(ConnectError::RoutingActivationDenied(
                        RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered
                    ))
                ),
                "{second:?}"
            );
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            first.close().await.expect("closed");
        })
        .await;
    });
}

/// A first connection alive-checked for a second's activation (ISO 13400-2:2019 REQ
/// 3.DoIP-091) that does not answer within `T_TCP_Alive_Check` is closed, and the second
/// takes its address (REQ 3.DoIP-092). The transport then routes to the second, and an
/// `ECUReset` there closes the second (ISO 14229-5:2022 REQ 7.11).
#[test]
fn a_silent_first_connection_gives_way_to_the_second() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut first = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );

            let mut second = tokio::select! {
                second = loopback.tester() => second.expect("activated"),
                // The clock moves in steps, giving the entity real time to act between
                // them, to 1.5 s: past `T_TCP_Alive_Check` once the check is sent, and
                // short of the second tester's own 2 s wait for its activation response.
                () = async {
                    for _ in 0..15 {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        advance(embassy_time::Duration::from_millis(100));
                    }
                    core::future::pending::<()>().await;
                } => unreachable!(),
            };
            assert_eq!(
                ask(&mut second, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            assert_eq!(next(&mut first).await, Then::Closed);

            assert_eq!(
                ask(&mut second, &[0x11, 0x01]).await,
                answered(&[0x51, 0x01])
            );
            assert_eq!(next(&mut second).await, Then::Closed);
        })
        .await;
    });
}

/// A sensor entity carrying messages of 64 bytes, so a response can outgrow it.
type SmallEntity = SensorEntity<64>;

/// The longest PDU the small entity carries.
const SMALL_PDU: usize = <SmallEntity as DiagnosticEntity>::MAX_PDU;

/// Two identifiers whose positive responses (`62`, the identifier, the record) are
/// exactly the small entity's limit and one byte over it, one whose read waits for
/// [`GATE`], and one with a one-byte record, eight of which fit one response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Did {
    Fits,
    Overflows,
    Gated,
    Short,
}

impl Did {
    const fn record_len(self) -> usize {
        match self {
            Self::Fits => SMALL_PDU - 3,
            Self::Overflows => SMALL_PDU - 2,
            Self::Gated | Self::Short => 1,
        }
    }
}

/// What a read of [`Did::Gated`] waits for.
static GATE: tokio::sync::Notify = tokio::sync::Notify::const_new();

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = SMALL_PDU - 2;
    fn as_u16(self) -> u16 {
        match self {
            Self::Fits => 0xF190,
            Self::Overflows => 0xF191,
            Self::Gated => 0xF192,
            Self::Short => 0xF193,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xF190 => Some(Self::Fits),
            0xF191 => Some(Self::Overflows),
            0xF192 => Some(Self::Gated),
            0xF193 => Some(Self::Short),
            _ => None,
        }
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        buf.split_at_checked(self.record_len())
            .ok_or(RecordError::Short)
    }
}

/// An application with a record too long for its entity, which reads up to eight
/// identifiers at once, so its requests can outgrow the server's concurrent buffer.
#[derive(Debug, Default)]
struct Reader;

impl ReadDataByIdentifier for Reader {
    type Did = Did;
    const MAY_RESPOND_PENDING: bool = false;
    const MAX_DIDS_PER_REQUEST: usize = 8;
    async fn read(&mut self, did: Did, out: &mut ResponseSink<'_>) -> Result<(), Nrc> {
        if did == Did::Gated {
            GATE.notified().await;
        }
        out.write_all(&vec![0xA5; did.record_len()])
            .map_err(|_| Nrc::ResponseTooLong)
    }
}

uds_server! {
    Reader: ReadDataByIdentifier;
    transport = DoIpTransport<SmallEntity, 2>,
    peers = 1,
    server = ReaderServer,
}

/// A response longer than the entity's [`DiagnosticEntity::MAX_PDU`] is answered
/// `responseTooLong` (ISO 14229-1:2020 Table A.1, NRC `0x14`), because the server's
/// response buffer is sized to what the entity carries (`UDSSVC_ARCH_0017`); a
/// response exactly at the limit is sent whole.
#[test]
fn a_response_longer_than_the_entity_carries_is_answered_0x14() {
    on_loopback(|loopback| async move {
        let mut server = reader(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let fits = ask(&mut tester, &[0x22, 0xF1, 0x90]).await;
            let Then::Response(_, response) = &fits.then else {
                panic!("{fits:?}");
            };
            assert_eq!(response.len(), SMALL_PDU);
            assert_eq!(response.get(..3), Some(&[0x62, 0xF1, 0x90][..]));

            assert_eq!(
                ask(&mut tester, &[0x22, 0xF1, 0x91]).await,
                answered(&[0x7F, 0x22, 0x14])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

fn reader(loopback: &Loopback) -> ReaderServer {
    ReaderServer::new(
        Reader,
        DoIpTransport::new(loopback.entity(), bench_reloads()),
        Address(ENTITY.0),
        PARAMS,
    )
}

/// The longest request [`ReaderServer`] accepts: its in-flight buffer.
fn reader_limit() -> usize {
    let mut store = <<Reader as ServiceSet>::Store as Storage>::EMPTY;
    store.split().in_flight.len()
}

/// `22` and `count` of `did`: a request `1 + 2 × count` bytes long.
fn read_of(did: Did, count: usize) -> Vec<u8> {
    let mut request = vec![0x22];
    request.extend(did.as_u16().to_be_bytes().repeat(count));
    request
}

/// A request longer than the server decodes, but within what the entity holds, is
/// acknowledged and answered by UDS in ISO 14229-1:2020's order, not refused by `DoIP`:
/// a service the server does not support `0x11`, one it does `0x13`, so an over-long
/// request for an unsupported service is not mistaken for a malformed one (#41).
#[test]
fn a_request_longer_than_the_server_decodes_is_answered_by_its_nrc() {
    on_loopback(|loopback| async move {
        let mut server = reader(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let too_long = read_of(Did::Short, reader_limit() / 2 + 1);
            assert!(too_long.len() > reader_limit());
            assert_eq!(
                ask(&mut tester, &too_long).await,
                answered(&[0x7F, 0x22, 0x13])
            );

            let mut unsupported = vec![0x2E, 0xF1, 0x90];
            unsupported.resize(reader_limit() + 1, 0x00);
            assert_eq!(
                ask(&mut tester, &unsupported).await,
                answered(&[0x7F, 0x2E, 0x11])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// The longest request the server decodes is served whole: its in-flight buffer holds
/// it, not one byte less.
#[test]
fn the_longest_request_the_server_decodes_is_served() {
    on_loopback(|loopback| async move {
        let mut server = reader(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let count = (reader_limit() - 1) / 2;
            let longest = read_of(Did::Short, count);
            assert_eq!(longest.len(), reader_limit());

            let mut response = vec![0x62];
            response.extend([0xF1, 0x93, 0xA5].repeat(count));
            assert_eq!(ask(&mut tester, &longest).await, answered(&response));
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A request the server would decode, but too long for the buffer it lends while a
/// service runs, is acknowledged and answered `busyRepeatRequest` (ISO 14229-1:2020 NRC
/// `0x21`), not `0x13`: it is turned away for the service in progress, not for its
/// length. The running service then answers.
#[test]
fn a_long_request_while_a_service_runs_is_answered_busy() {
    on_loopback(|loopback| async move {
        let mut server = reader(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            assert_eq!(send(&mut tester, &[0x22, 0xF1, 0x92]).await, DoIpResult::Ok);

            let long = read_of(Did::Short, (reader_limit() - 1) / 2);
            assert_eq!(ask(&mut tester, &long).await, answered(&[0x7F, 0x22, 0x21]));

            GATE.notify_one();
            assert_eq!(
                next(&mut tester).await,
                Then::Response(ENTITY, vec![0x62, 0xF1, 0x92, 0xA5])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}
