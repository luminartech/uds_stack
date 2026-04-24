//! Verifies that TesterPresent is emitted at the configured interval while the
//! client is holding the `doip_client` lock through a long NRC 0x78
//! (Response Pending) wait. Prior to the in-request keepalive, the background
//! task was starved on the same mutex, so TesterPresent stopped firing for the
//! duration of slow operations like a flash-memory erase.

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

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
            tp_counter.fetch_add(1, Ordering::SeqCst);
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
                                tp_counter.fetch_add(1, Ordering::SeqCst);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tester_present_fires_during_nrc78_wait() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("failed to bind listener");
    let port = listener.local_addr().unwrap().port();

    let tp_counter = Arc::new(AtomicUsize::new(0));
    let server_tp = Arc::clone(&tp_counter);
    // 5 pending rounds × 400 ms ≈ 2 s of wait. With a 200 ms interval we expect
    // ~10 TPs; we assert at least 4 to leave generous scheduling slack.
    let server_task = tokio::spawn(async move {
        run_fake_server(listener, server_tp, 5, Duration::from_millis(400)).await;
    });

    let client = connect_client(
        port,
        SessionConfig {
            tester_present_interval: Duration::from_millis(200),
            auto_tester_present: true,
            auto_reconnect: false,
            response_timeout: Duration::from_secs(30),
            ..SessionConfig::default()
        },
    )
    .await;

    // Count TPs that arrived *after* the slow request starts, so the pre-request
    // keepalive-task sends (if any) don't confuse the assertion.
    let tp_before = tp_counter.load(Ordering::SeqCst);

    let response = timeout(
        Duration::from_secs(20),
        client.send_raw(UDS_SLOW_REQ.to_vec()),
    )
    .await
    .expect("send_raw timed out")
    .expect("send_raw failed");
    assert_eq!(response, UDS_SLOW_FINAL_RSP);

    // Give the server a beat to flush any final ACKs before tearing down.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let tp_after = tp_counter.load(Ordering::SeqCst);
    let during = tp_after - tp_before;

    drop(client);
    let _ = server_task.await;

    assert!(
        during >= 4,
        "expected at least 4 TesterPresent messages during the ~2s NRC 0x78 wait, got {during} (total {tp_after})",
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
    let total = tp_counter.load(Ordering::SeqCst);
    drop(client);
    let _ = server_task.await;

    assert_eq!(
        total, 0,
        "no TesterPresent should be sent when auto_tester_present is false, got {total}",
    );
}
