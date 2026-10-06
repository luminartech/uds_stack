//! A late acknowledgement over real loopback sockets, through `edge-nal-std`, where a
//! socket reports data already received only after a reactor turn: the case a scripted
//! backend, which reports it at once, cannot show.
//!
//! A test binary of its own because it moves `embassy-time`'s mock clock, which is
//! process-wide.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration as StdDuration;

use edge_nal_std::Stack;
use embassy_time::{Duration, MockDriver};
use simple_doip::service::{ConnectionEvent, DiagnosticConnection, DoIpResult};
use simple_doip::tester::{Tester, TesterAddress};
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

/// The entity: activates routing, then acknowledges and answers two requests, holding
/// the first acknowledgement back until `release` says the tester has given up on it.
fn entity(
    listener: &TcpListener,
    received: &mpsc::Sender<()>,
    release: &mpsc::Receiver<()>,
) {
    let (mut socket, _) = listener.accept().unwrap();
    let mut activation = [0; 15];
    socket.read_exact(&mut activation).unwrap();
    socket.write_all(&ACTIVATION_RESPONSE).unwrap();

    let mut first = [0; 14];
    socket.read_exact(&mut first).unwrap();
    received.send(()).unwrap();
    release.recv().unwrap();
    socket.write_all(&ACK).unwrap();
    socket.write_all(&response(&[0x7E, 0x00])).unwrap();

    let mut second = [0; 15];
    socket.read_exact(&mut second).unwrap();
    socket.write_all(&ACK).unwrap();
    socket.write_all(&response(&[0x62, 0xF1, 0x90])).unwrap();
    let mut rest = Vec::new();
    socket.read_to_end(&mut rest).ok();
}

/// ISO 13400-2:2019 Table 12: a request the caller comes back to after
/// `A_DoIP_Diagnostic_Message` is lost, even though its acknowledgement already sits in
/// the socket; that acknowledgement is then discarded, and the next request is confirmed
/// by its own.
#[tokio::test(flavor = "current_thread")]
async fn a_late_acknowledgement_does_not_confirm_the_next_request() {
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
        .await
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

    let event = timeout(PATIENCE, tester.next_event(&mut buf, None))
        .await
        .unwrap();
    assert_eq!(
        event.unwrap(),
        ConnectionEvent::Confirm {
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            result: DoIpResult::TimeoutA,
        }
    );

    tester
        .request(ENTITY, TaType::Physical, &[0x22, 0xF1, 0x90])
        .await
        .unwrap();
    let mut events = Vec::new();
    while events.len() < 3 {
        let event = timeout(PATIENCE, tester.next_event(&mut buf, None))
            .await
            .unwrap()
            .unwrap();
        events.push(format!("{event:?}"));
    }
    assert_eq!(
        events,
        [
            format!(
                "{:?}",
                ConnectionEvent::Indication {
                    sa: ENTITY,
                    ta: TESTER,
                    ta_type: TaType::Physical,
                    pdu: &[0x7E, 0x00],
                }
            ),
            format!(
                "{:?}",
                ConnectionEvent::Confirm {
                    sa: TESTER,
                    ta: ENTITY,
                    ta_type: TaType::Physical,
                    result: DoIpResult::Ok,
                }
            ),
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
