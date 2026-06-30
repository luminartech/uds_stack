//! Verifies TesterPresent keepalive behavior around NRC 0x78 (Response
//! Pending) waits. Key invariants:
//!
//! * TP is suppressed while the ECU is actively sending NRC 0x78 responses —
//!   the ECU is demonstrably alive and keeps the S3 session timer itself.
//! * A stray TP NACK that arrives during a pending wait (e.g. the ECU
//!   responding to a TP sent before the NRC 0x78 cycle began) must be ignored
//!   rather than aborting the primary request.

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

// Counters only track aggregate "did this happen" state, so Relaxed is
// sufficient across the threads that touch them.
const COUNTER_ORDER: Ordering = Ordering::Relaxed;

use async_trait::async_trait;
use simple_doip::{
    LogicalAddress,
    client::{Client as DoipClient, ClientOptions, RoutingActivationOptions},
    connection::Connector,
    messages::{ActivationTypeCode, ProtocolVersion},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
    time::timeout,
};
use uds_on_ip::{SessionConfig, UdsClient};

const SERVER_PHYSICAL: LogicalAddress = LogicalAddress(0x4010);
// Must be in the valid client address range 0x0E00-0x0FFF enforced by simple_doip.
const CLIENT_LA: LogicalAddress = LogicalAddress(0x0E00);

const PT_ROUTING_ACTIVATION_REQ: u16 = 0x0005;
const PT_ROUTING_ACTIVATION_RSP: u16 = 0x0006;
const PT_DIAG_MESSAGE: u16 = 0x8001;
const PT_DIAG_ACK: u16 = 0x8002;

const UDS_SLOW_REQ: &[u8] = &[0x22, 0xF1, 0x5A];
const UDS_SLOW_FINAL_RSP: &[u8] = &[0x62, 0xF1, 0x5A, 0x42];
const UDS_TESTER_PRESENT: &[u8] = &[0x3E, 0x80];

/// Read the next full DoIP frame from `stream`. Returns `(payload_type, payload)`
/// or `None` if the peer closed cleanly.
async fn read_frame(stream: &mut TcpStream) -> Option<(u16, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await.ok()?;
    assert_eq!(header[0], 0x02, "bad DoIP version byte");
    assert_eq!(header[1], 0xFD, "bad DoIP inverse version byte");
    let payload_type = u16::from_be_bytes([header[2], header[3]]);
    let payload_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload).await.ok()?;
    Some((payload_type, payload))
}

fn build_frame(payload_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&[0x02, 0xFD]);
    out.extend_from_slice(&payload_type.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn build_diag_ack(source: LogicalAddress, target: LogicalAddress, previous: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5 + previous.len());
    payload.extend_from_slice(&source.0.to_be_bytes());
    payload.extend_from_slice(&target.0.to_be_bytes());
    payload.push(0x00); // ack code: positive ack
    payload.extend_from_slice(previous);
    build_frame(PT_DIAG_ACK, &payload)
}

fn build_diag_msg(source: LogicalAddress, target: LogicalAddress, user_data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + user_data.len());
    payload.extend_from_slice(&source.0.to_be_bytes());
    payload.extend_from_slice(&target.0.to_be_bytes());
    payload.extend_from_slice(user_data);
    build_frame(PT_DIAG_MESSAGE, &payload)
}

fn build_routing_activation_rsp(tester: LogicalAddress, entity: LogicalAddress) -> Vec<u8> {
    let mut payload = Vec::with_capacity(9);
    payload.extend_from_slice(&tester.0.to_be_bytes());
    payload.extend_from_slice(&entity.0.to_be_bytes());
    payload.push(0x10); // RoutingSuccessfullyActivated
    payload.extend_from_slice(&[0, 0, 0, 0]); // reserved
    build_frame(PT_ROUTING_ACTIVATION_RSP, &payload)
}

/// Runs a fake DoIP server that:
/// 1. Completes routing activation.
/// 2. Answers any request other than UDS_SLOW_REQ with a dummy positive response.
/// 3. On receiving UDS_SLOW_REQ, sends a DoIP ack, then `num_pending` NRC 0x78
///    responses at `pending_interval` apart, then UDS_SLOW_FINAL_RSP.
/// 4. Throughout, counts incoming TesterPresent (0x3E 0x80) messages in
///    `tp_counter` and always ACKs them.
async fn run_fake_server(
    listener: TcpListener,
    tp_counter: Arc<AtomicUsize>,
    num_pending: usize,
    pending_interval: Duration,
) {
    let (mut stream, _) = listener
        .accept()
        .await
        .expect("fake server failed to accept");

    // Step 1: routing activation.
    let (pt, payload) = read_frame(&mut stream)
        .await
        .expect("client closed before routing activation");
    assert_eq!(
        pt, PT_ROUTING_ACTIVATION_REQ,
        "expected routing activation request"
    );
    let tester_addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    stream
        .write_all(&build_routing_activation_rsp(tester_addr, SERVER_PHYSICAL))
        .await
        .expect("failed to send activation response");

    // Step 2: main loop. Handle incoming UDS requests.
    loop {
        let Some((pt, payload)) = read_frame(&mut stream).await else {
            return;
        };
        if pt != PT_DIAG_MESSAGE || payload.len() < 4 {
            continue;
        }
        let client_addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
        let _server_addr = LogicalAddress(u16::from_be_bytes([payload[2], payload[3]]));
        let user_data = &payload[4..];

        // Always ack, like a real DoIP stack.
        stream
            .write_all(&build_diag_ack(SERVER_PHYSICAL, client_addr, user_data))
            .await
            .expect("failed to send diag ack");

        if user_data == UDS_TESTER_PRESENT {
            tp_counter.fetch_add(1, COUNTER_ORDER);
            // TP is sent with suppress-positive-response (0x80), no UDS response.
            continue;
        }

        if user_data == UDS_SLOW_REQ {
            // Send NRC 0x78 (Response Pending) at the configured cadence, while
            // continuing to ACK any TesterPresent that arrives during the wait.
            for _ in 0..num_pending {
                let deadline = tokio::time::Instant::now() + pending_interval;
                loop {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match timeout(remaining, read_frame(&mut stream)).await {
                        Ok(Some((PT_DIAG_MESSAGE, pl))) if pl.len() >= 4 => {
                            let tp_client_addr = LogicalAddress(u16::from_be_bytes([pl[0], pl[1]]));
                            let tp_user = &pl[4..];
                            stream
                                .write_all(&build_diag_ack(
                                    SERVER_PHYSICAL,
                                    tp_client_addr,
                                    tp_user,
                                ))
                                .await
                                .expect("failed to ack TP during pending");
                            if tp_user == UDS_TESTER_PRESENT {
                                tp_counter.fetch_add(1, COUNTER_ORDER);
                            }
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => return,
                        Err(_) => break, // deadline reached
                    }
                }
                stream
                    .write_all(&build_diag_msg(
                        SERVER_PHYSICAL,
                        client_addr,
                        &[0x7F, UDS_SLOW_REQ[0], 0x78],
                    ))
                    .await
                    .expect("failed to send NRC 0x78");
            }

            // Final positive response.
            stream
                .write_all(&build_diag_msg(
                    SERVER_PHYSICAL,
                    client_addr,
                    UDS_SLOW_FINAL_RSP,
                ))
                .await
                .expect("failed to send final response");
            continue;
        }

        // Any other request: respond with a generic positive echo.
        let mut rsp = vec![user_data[0] + 0x40];
        rsp.extend_from_slice(&user_data[1..]);
        stream
            .write_all(&build_diag_msg(SERVER_PHYSICAL, client_addr, &rsp))
            .await
            .expect("failed to send dummy response");
    }
}

/// A test-only [`Connector`] that connects to the DoIP server without the port
/// 13400 restriction enforced by the production `ConnectorSocket`. Needed
/// because our fake server binds to an ephemeral port so tests can run in
/// parallel and don't require privileged ports.
struct LoopbackConnector;

#[async_trait]
impl Connector for LoopbackConnector {
    async fn establish_connection(
        gateway_address: std::net::SocketAddr,
    ) -> Result<
        (
            tokio::net::tcp::OwnedReadHalf,
            tokio::net::tcp::OwnedWriteHalf,
        ),
        simple_doip::Error,
    > {
        let tcp_socket = match gateway_address {
            std::net::SocketAddr::V4(_) => TcpSocket::new_v4().unwrap(),
            std::net::SocketAddr::V6(_) => TcpSocket::new_v6().unwrap(),
        };
        tcp_socket.set_nodelay(true).unwrap();
        let tcp_stream = timeout(Duration::from_secs(2), tcp_socket.connect(gateway_address))
            .await
            .map_err(simple_doip::Error::ConnectionTimeout)?
            .map_err(simple_doip::Error::NetworkError)?;
        Ok(tcp_stream.into_split())
    }
}

/// Build a `UdsClient` connected to a fake server on `port`, with the supplied
/// session config. Uses the loopback connector so the ephemeral port works.
async fn connect_client(port: u16, session_config: SessionConfig) -> UdsClient<LoopbackConnector> {
    let options = ClientOptions {
        server_address: std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        server_logical_address: SERVER_PHYSICAL,
        server_physical_address: SERVER_PHYSICAL,
        client_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        client_logical_address: CLIENT_LA,
        protocol_version: ProtocolVersion::V2012,
        routing_activation_options: Some(RoutingActivationOptions {
            activation_type: ActivationTypeCode::Default,
            oem_specific: None,
        }),
    };
    let doip_client = DoipClient::<LoopbackConnector>::connect(options)
        .await
        .expect("doip client connect failed");
    UdsClient::from_doip_client(doip_client, session_config)
}

/// Once the ECU has sent at least one NRC 0x78, the client must suppress
/// in-loop TesterPresent sends. The ECU is actively processing and its
/// NRC 0x78 responses keep the S3 session timer alive — injecting TP
/// during a long operation (e.g. flash erase) can disrupt the cadence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tester_present_suppressed_during_nrc78_wait() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let tp_counter = Arc::new(AtomicUsize::new(0));
    let server_tp = Arc::clone(&tp_counter);
    // 4 pending rounds × 100 ms = ~400 ms of wait. The first NRC 0x78
    // arrives at ~100 ms, before the 500 ms TP interval elapses, so
    // pending_count > 0 suppresses all TP sends for the entire wait.
    // Without suppression ~2 TPs would fire; we assert zero.
    let server_task = tokio::spawn(async move {
        run_fake_server(listener, server_tp, 4, Duration::from_millis(100)).await;
    });

    let client = connect_client(
        port,
        SessionConfig {
            tester_present_interval: Duration::from_millis(500),
            auto_tester_present: true,
            auto_reconnect: false,
            response_timeout: Duration::from_secs(30),
            ..SessionConfig::default()
        },
    )
    .await;

    let tp_before = tp_counter.load(COUNTER_ORDER);

    let response = timeout(
        Duration::from_secs(30),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out")
    .expect("send_raw failed");
    assert_eq!(response, UDS_SLOW_FINAL_RSP);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let during = tp_counter.load(COUNTER_ORDER) - tp_before;

    drop(client);
    let _ = server_task.await;

    assert_eq!(
        during, 0,
        "TesterPresent must be suppressed while ECU is sending NRC 0x78, got {during}",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tester_present_not_sent_when_disabled() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let tp_counter = Arc::new(AtomicUsize::new(0));
    let server_tp = Arc::clone(&tp_counter);
    let server_task = tokio::spawn(async move {
        run_fake_server(listener, server_tp, 3, Duration::from_millis(300)).await;
    });

    let client = connect_client(
        port,
        SessionConfig {
            auto_tester_present: false,
            auto_reconnect: false,
            tester_present_interval: Duration::from_millis(100),
            response_timeout: Duration::from_secs(30),
            ..SessionConfig::default()
        },
    )
    .await;

    let response = timeout(
        Duration::from_secs(20),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out")
    .expect("send_raw failed");
    assert_eq!(response, UDS_SLOW_FINAL_RSP);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let total = tp_counter.load(COUNTER_ORDER);
    drop(client);
    let _ = server_task.await;

    assert_eq!(
        total, 0,
        "no TesterPresent should be sent when auto_tester_present is false, got {total}",
    );
}

/// Even with `auto_tester_present: true`, a zero `tester_present_interval`
/// must not produce a busy-send loop in the receive path. With the helper
/// properly guarding zero intervals, the expected TP count during the wait
/// is exactly 0, and the slow request still completes normally.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tester_present_not_sent_when_interval_is_zero() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let tp_counter = Arc::new(AtomicUsize::new(0));
    let server_tp = Arc::clone(&tp_counter);
    let server_task = tokio::spawn(async move {
        run_fake_server(listener, server_tp, 3, Duration::from_millis(200)).await;
    });

    let client = connect_client(
        port,
        SessionConfig {
            auto_tester_present: true,
            auto_reconnect: false,
            tester_present_interval: Duration::ZERO,
            response_timeout: Duration::from_secs(30),
            ..SessionConfig::default()
        },
    )
    .await;

    let response = timeout(
        Duration::from_secs(10),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out — possible busy-spin on zero interval")
    .expect("send_raw failed");
    assert_eq!(response, UDS_SLOW_FINAL_RSP);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let total = tp_counter.load(COUNTER_ORDER);
    drop(client);
    let _ = server_task.await;

    assert_eq!(
        total, 0,
        "zero interval must disable TP emission, got {total}",
    );
}

// TODO (coverage gap): an end-to-end test for the "reconnect during an
// in-request NRC 0x78 wait then emit a TP on the new connection" path
// would exercise `reconnect_and_keepalive` under test. It requires an
// `auto_reconnect: true` client and a fake server that implements DoIP's
// listener-side reconnect (see `simple_doip::client::Client::reconnect`
// and `bind_socket`), which is a substantial amount of plumbing beyond
// this test's scope. Behavior is verified today via the live flash on a
// real sensor (see the #543 test report) and implicitly by the
// `nrc78_cap_aborts_stuck_server` test below, which exercises the same
// receive loop without reconnect.

/// A server stuck in NRC 0x78 forever exercises
/// `max_response_pending_count`. With a small cap the client must abort
/// the request with `Error::Nrc78PendingExceeded` rather than waiting
/// indefinitely.
async fn run_forever_pending_server(listener: TcpListener) {
    let (mut stream, _) = listener.accept().await.expect("accept failed");
    let (pt, payload) = read_frame(&mut stream)
        .await
        .expect("missing routing activation");
    assert_eq!(pt, PT_ROUTING_ACTIVATION_REQ);
    let tester = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    stream
        .write_all(&build_routing_activation_rsp(tester, SERVER_PHYSICAL))
        .await
        .expect("write activation rsp");

    let (pt, payload) = read_frame(&mut stream).await.expect("missing slow req");
    assert_eq!(pt, PT_DIAG_MESSAGE);
    let addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    let user_data = &payload[4..];
    stream
        .write_all(&build_diag_ack(SERVER_PHYSICAL, addr, user_data))
        .await
        .expect("write diag ack");

    // Infinite NRC 0x78 loop. Ack any TP that arrives in between.
    loop {
        stream
            .write_all(&build_diag_msg(
                SERVER_PHYSICAL,
                addr,
                &[0x7F, UDS_SLOW_REQ[0], 0x78],
            ))
            .await
            .expect("write NRC 0x78");
        let sleep_until = tokio::time::Instant::now() + Duration::from_millis(150);
        loop {
            let remaining = sleep_until.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match timeout(remaining, read_frame(&mut stream)).await {
                Ok(Some((PT_DIAG_MESSAGE, pl))) if pl.len() >= 4 => {
                    let a = LogicalAddress(u16::from_be_bytes([pl[0], pl[1]]));
                    let _ = stream
                        .write_all(&build_diag_ack(SERVER_PHYSICAL, a, &pl[4..]))
                        .await;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    }
}

/// After an NRC 0x78 (Response Pending) the client must widen its wait
/// from P2 (`response_timeout`) to P2* (`response_pending_timeout`) per
/// ISO 14229. This test sets P2 = 400 ms and P2* = 3 s and has the
/// server reply with one NRC 0x78 at ~200 ms then the final response at
/// ~1.2 s — well past P2 but inside P2*. If the client failed to switch
/// timers it would either time out at 400 ms or, with reconnect off,
/// fall into `Err(Timeout)`.
async fn run_p2_star_server(listener: TcpListener) {
    let (mut stream, _) = listener.accept().await.expect("accept failed");
    let (pt, payload) = read_frame(&mut stream)
        .await
        .expect("missing routing activation");
    assert_eq!(pt, PT_ROUTING_ACTIVATION_REQ);
    let tester = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    stream
        .write_all(&build_routing_activation_rsp(tester, SERVER_PHYSICAL))
        .await
        .expect("write activation rsp");

    let (pt, payload) = read_frame(&mut stream).await.expect("missing slow req");
    assert_eq!(pt, PT_DIAG_MESSAGE);
    let addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    let user_data = &payload[4..];
    stream
        .write_all(&build_diag_ack(SERVER_PHYSICAL, addr, user_data))
        .await
        .expect("write diag ack");

    // NRC 0x78 within P2 (200 ms after request).
    tokio::time::sleep(Duration::from_millis(200)).await;
    stream
        .write_all(&build_diag_msg(
            SERVER_PHYSICAL,
            addr,
            &[0x7F, UDS_SLOW_REQ[0], 0x78],
        ))
        .await
        .expect("write NRC 0x78");

    // Final response ~1 s later — past P2 (400 ms) but within P2* (3 s).
    tokio::time::sleep(Duration::from_millis(1000)).await;
    stream
        .write_all(&build_diag_msg(SERVER_PHYSICAL, addr, UDS_SLOW_FINAL_RSP))
        .await
        .expect("write final rsp");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nrc78_widens_timeout_from_p2_to_p2_star() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(run_p2_star_server(listener));

    let client = connect_client(
        port,
        SessionConfig {
            tester_present_interval: Duration::from_millis(500),
            auto_tester_present: true,
            auto_reconnect: false,
            // Short P2: a client that fails to switch to P2* after NRC 0x78
            // will time out waiting for the final response.
            response_timeout: Duration::from_millis(400),
            response_pending_timeout: Duration::from_secs(3),
            ..SessionConfig::default()
        },
    )
    .await;

    let response = timeout(
        Duration::from_secs(5),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw outer timeout — NRC 0x78 likely did not widen timeout to P2*")
    .expect("send_raw failed — NRC 0x78 likely did not widen timeout to P2*");
    assert_eq!(response, UDS_SLOW_FINAL_RSP);

    drop(client);
    let _ = server_task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nrc78_cap_aborts_stuck_server() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(run_forever_pending_server(listener));

    let max = 5u32;
    let client = connect_client(
        port,
        SessionConfig {
            tester_present_interval: Duration::from_millis(500),
            auto_tester_present: true,
            auto_reconnect: false,
            response_timeout: Duration::from_secs(10),
            max_response_pending_count: Some(max),
            ..SessionConfig::default()
        },
    )
    .await;

    let err = timeout(
        Duration::from_secs(10),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out instead of returning Nrc78PendingExceeded")
    .expect_err("expected error but got positive response");

    match err {
        uds_on_ip::Error::Nrc78PendingExceeded { max: reported_max } => {
            assert_eq!(reported_max, max);
        }
        other => panic!("expected Nrc78PendingExceeded, got {other:?}"),
    }

    drop(client);
    server_task.abort();
    let _ = server_task.await;
}

/// Fake server that spontaneously injects a stray TP NACK during a
/// UDS_SLOW_REQ pending wait (simulating the ECU responding to a TP
/// that was sent before the NRC 0x78 cycle began), then delivers the
/// final positive response. Exercises the `classify_response` fix —
/// the stray NACK for SID 0x3E must not abort the UDS_SLOW_REQ.
async fn run_server_that_nacks_tp_during_pending(listener: TcpListener) {
    let (mut stream, _) = listener
        .accept()
        .await
        .expect("fake server failed to accept");

    // Routing activation.
    let (pt, payload) = read_frame(&mut stream)
        .await
        .expect("client closed before routing activation");
    assert_eq!(pt, PT_ROUTING_ACTIVATION_REQ);
    let tester_addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    stream
        .write_all(&build_routing_activation_rsp(tester_addr, SERVER_PHYSICAL))
        .await
        .expect("failed to send activation response");

    // Primary request.
    let (pt, payload) = read_frame(&mut stream)
        .await
        .expect("client closed before UDS_SLOW_REQ");
    assert_eq!(pt, PT_DIAG_MESSAGE);
    let client_addr = LogicalAddress(u16::from_be_bytes([payload[0], payload[1]]));
    let user_data = &payload[4..];
    assert_eq!(user_data, UDS_SLOW_REQ, "expected UDS_SLOW_REQ first");
    stream
        .write_all(&build_diag_ack(SERVER_PHYSICAL, client_addr, user_data))
        .await
        .expect("failed to ack UDS_SLOW_REQ");

    // NRC 0x78 to push the client into the pending wait.
    stream
        .write_all(&build_diag_msg(
            SERVER_PHYSICAL,
            client_addr,
            &[0x7F, UDS_SLOW_REQ[0], 0x78],
        ))
        .await
        .expect("failed to send NRC 0x78");

    // Spontaneously inject a stray TP NACK — simulates the ECU
    // responding to a TesterPresent that was sent before the NRC 0x78
    // cycle began. This arrives on the same TCP stream as the primary
    // response and must be ignored by the client.
    tokio::time::sleep(Duration::from_millis(50)).await;
    stream
        .write_all(&build_diag_msg(
            SERVER_PHYSICAL,
            client_addr,
            &[0x7F, 0x3E, 0x22],
        ))
        .await
        .expect("failed to send stray TP NACK");

    // Final positive response for the primary request.
    stream
        .write_all(&build_diag_msg(
            SERVER_PHYSICAL,
            client_addr,
            UDS_SLOW_FINAL_RSP,
        ))
        .await
        .expect("failed to send final response");

    loop {
        if read_frame(&mut stream).await.is_none() {
            return;
        }
    }
}

/// Regression: a stray NACK for an in-flight TesterPresent (0x3E) that
/// arrives on the same TCP stream while the client is waiting on the
/// primary request's response must be **ignored**, not treated as a
/// failed primary response.
///
/// Pre-fix, this produced `Error::InvalidResponse("Negative response
/// for SID 0x3e but expected 0x22")` and aborted the [INTERNAL_PROJECT_REDACTED] run (observed
/// at `[8/37] Error: Command failed: ...` in the reported [INTERNAL_PROJECT_REDACTED] log).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stray_tp_nack_during_pending_wait_is_ignored() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(run_server_that_nacks_tp_during_pending(listener));

    let client = connect_client(
        port,
        SessionConfig {
            tester_present_interval: Duration::from_millis(200),
            auto_tester_present: true,
            auto_reconnect: false,
            response_timeout: Duration::from_secs(2),
            response_pending_timeout: Duration::from_secs(10),
            ..SessionConfig::default()
        },
    )
    .await;

    let response = timeout(
        Duration::from_secs(10),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out — stray TP NACK likely aborted the primary request")
    .expect("send_raw failed — stray TP NACK should not fail the primary request");

    assert_eq!(
        response,
        UDS_SLOW_FINAL_RSP.to_vec(),
        "primary request must return the final positive response, not the stray TP NACK",
    );

    drop(client);
    server_task.abort();
    let _ = server_task.await;
}
