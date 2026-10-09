//! A request lost to `A_DoIP_Diagnostic_Message` over real loopback sockets, through
//! `edge-nal-std`, whose acknowledgement and response were already in the socket: the
//! retry on a new connection sees only its own.
//!
//! A test binary of its own because it moves `embassy-time`'s mock clock, which is
//! process-wide.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration as StdDuration;

use edge_nal_std::Stack;
use embassy_time::{Duration, MockDriver};
use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, TesterAddress, TesterConnection,
};
use simple_doip::tester::{RECONNECT_BACKOFF, Tester};
use simple_doip::{LogicalAddress, TaType};
use tokio::time::timeout;

const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const ENTITY: LogicalAddress = LogicalAddress(0x0001);

/// A real-time bound on each step, so a lost message fails the test rather than hang it.
const PATIENCE: StdDuration = StdDuration::from_secs(5);

const ACTIVATION_RESPONSE: [u8; 17] = [
    0x03, 0xFC, 0x00, 0x06, 0, 0, 0, 9, 0x0E, 0x00, 0x00, 0x01, 0x10, 0, 0, 0, 0,
];
const ACK: [u8; 13] = [
    0x03, 0xFC, 0x80, 0x02, 0, 0, 0, 5, 0x00, 0x01, 0x0E, 0x00, 0x00,
];

fn response(pdu: &[u8]) -> Vec<u8> {
    let length = u8::try_from(pdu.len()).unwrap().checked_add(4).unwrap();
    let mut frame = vec![0x03, 0xFC, 0x80, 0x01, 0, 0, 0, length];
    frame.extend([0x00, 0x01, 0x0E, 0x00]);
    frame.extend(pdu);
    frame
}

fn activated(listener: &TcpListener) -> TcpStream {
    let (mut socket, _) = listener.accept().unwrap();
    let mut activation = [0; 15];
    socket.read_exact(&mut activation).unwrap();
    socket.write_all(&ACTIVATION_RESPONSE).unwrap();
    socket
}

/// The entity: on the first connection it acknowledges and answers the request only once
/// `release` says the tester has given it up; on the second it answers at once.
fn entity(
    listener: &TcpListener,
    received: &mpsc::Sender<()>,
    release: &mpsc::Receiver<()>,
) {
    let mut first = activated(listener);
    let mut request = [0; 14];
    first.read_exact(&mut request).unwrap();
    received.send(()).unwrap();
    release.recv().unwrap();
    first.write_all(&ACK).ok();
    first.write_all(&response(&[0x7E, 0x00])).ok();

    let mut second = activated(listener);
    let mut request = [0; 15];
    second.read_exact(&mut request).unwrap();
    second.write_all(&ACK).unwrap();
    second.write_all(&response(&[0x62, 0xF1, 0x90])).unwrap();
    let mut rest = Vec::new();
    second.read_to_end(&mut rest).ok();
}

/// ISO 13400-2:2019 Table 12: a request the caller comes back to after
/// `A_DoIP_Diagnostic_Message` is lost, even though its acknowledgement already sits in
/// the socket. The tester gives the connection up, so the next request, on a new
/// connection, is confirmed and answered by its own.
#[tokio::test(flavor = "current_thread")]
async fn a_lost_request_leaves_nothing_for_the_next_one() {
    MockDriver::get().reset();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote = listener.local_addr().unwrap();
    let (received, request_received) = mpsc::channel();
    let (release_ack, release) = mpsc::channel();
    let peer = std::thread::spawn(move || entity(&listener, &received, &release));
    let stack = Stack::new();
    let sa = TesterAddress::new(TESTER).unwrap();
    let mut tester = timeout(PATIENCE, Tester::<_, 64>::connect(&stack, remote, sa))
        .await
        .unwrap()
        .unwrap();
    let mut buf = [0; 16];

    tester
        .request(ENTITY, TaType::Physical, &[0x3E, 0x00])
        .unwrap();
    let writing = timeout(
        StdDuration::from_millis(200),
        tester.next_event(&mut buf, None),
    );
    assert!(writing.await.is_err(), "nothing answers the request yet");
    request_received.recv_timeout(PATIENCE).unwrap();
    release_ack.send(()).unwrap();
    std::thread::sleep(StdDuration::from_millis(100));
    MockDriver::get().advance(Duration::from_secs(3));

    let mut events = Vec::new();
    for _ in 0..2 {
        let event = timeout(PATIENCE, tester.next_event(&mut buf, None))
            .await
            .unwrap()
            .unwrap();
        events.push(format!("{event:?}"));
    }
    MockDriver::get().advance(RECONNECT_BACKOFF);
    timeout(PATIENCE, tester.reconnect(None))
        .await
        .unwrap()
        .unwrap();
    tester
        .request(ENTITY, TaType::Physical, &[0x22, 0xF1, 0x90])
        .unwrap();
    for _ in 0..2 {
        let event = timeout(PATIENCE, tester.next_event(&mut buf, None))
            .await
            .unwrap()
            .unwrap();
        events.push(format!("{event:?}"));
    }

    let confirm = |result| ConnectionEvent::Confirm {
        sa: TESTER,
        ta: ENTITY,
        ta_type: TaType::Physical,
        result,
    };
    assert_eq!(
        events,
        [
            format!("{:?}", confirm(DoIpResult::TimeoutA)),
            format!("{:?}", ConnectionEvent::Closed),
            format!("{:?}", confirm(DoIpResult::Ok)),
            format!(
                "{:?}",
                ConnectionEvent::Indication {
                    sa: ENTITY,
                    ta: TESTER,
                    ta_type: TaType::Physical,
                    pdu: &[0x62, 0xF1, 0x90],
                }
            ),
        ]
    );
    drop(tester);
    peer.join().unwrap();
}
