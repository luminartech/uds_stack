//! `Entity` over real loopback sockets through `edge-nal-std`, against this crate's
//! `Tester` and plain TCP clients.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used, clippy::panic)]

mod support;

use std::net::SocketAddr;

use edge_nal::TcpBind;
use edge_nal_std::Stack;
use simple_doip::entity::{Entity, EntityAddress};
use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, DiagnosticEntity, DoIpResult, EntityEvent,
    TesterAddress,
};
use simple_doip::tester::Tester;
use simple_doip::{LogicalAddress, TaType};
use support::the_tester;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};

const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const ENTITY: LogicalAddress = LogicalAddress(0x0001);

/// A real-time bound on each exchange. The entity's and tester's timers run on
/// `embassy-time`'s mock clock here, which never advances, so a lost message would
/// otherwise hang.
const PATIENCE: Duration = Duration::from_secs(5);

/// A loopback address with a port nothing is listening on.
fn free_local_address() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn address() -> EntityAddress {
    EntityAddress::new(ENTITY, LogicalAddress(0xE400)).unwrap()
}

/// Answers every indication by echoing it, until `answers` have been confirmed.
async fn echo<E: DiagnosticEntity>(entity: &mut E, answers: usize) {
    let mut confirmed = 0;
    while confirmed < answers {
        let mut buf = [0u8; 64];
        match entity.next_event(&mut buf, None).await.unwrap() {
            EntityEvent::Indication { sa, pdu, .. } => {
                let mut answer = pdu.to_vec();
                if let Some(sid) = answer.first_mut() {
                    *sid |= 0x40;
                }
                entity
                    .request(ENTITY, sa, TaType::Physical, &answer)
                    .await
                    .unwrap();
            }
            EntityEvent::Confirm { result, .. } => {
                assert_eq!(result, DoIpResult::Ok);
                confirmed = confirmed.checked_add(1).unwrap();
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// A `Tester` connects, activates routing and is answered, end to end over TCP.
#[tokio::test(flavor = "current_thread")]
async fn a_tester_is_answered_over_loopback() {
    let local = free_local_address();
    let stack = Stack::new();
    let acceptor = stack.bind(local).await.unwrap();
    let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address(), the_tester());

    let tester = Box::pin(async {
        let mut tester =
            Tester::<_, 4108>::connect(&stack, local, TesterAddress::new(TESTER).unwrap())
                .await
                .unwrap();
        tester
            .request(ENTITY, TaType::Physical, &[0x3E, 0x00])
            .await
            .unwrap();
        let mut buf = [0u8; 64];
        loop {
            match tester.next_event(&mut buf, None).await.unwrap() {
                ConnectionEvent::Confirm { result, .. } => {
                    assert_eq!(result, DoIpResult::Ok);
                }
                ConnectionEvent::Indication { pdu, .. } => return pdu.to_vec(),
                other => panic!("unexpected {other:?}"),
            }
        }
    });

    let ((), answer) = timeout(PATIENCE, async {
        tokio::join!(Box::pin(echo(&mut entity, 1)), tester)
    })
    .await
    .unwrap();
    assert_eq!(answer, [0x7E, 0x00]);
}

/// A connection beyond the `MCTS + 1` sockets the entity holds is accepted and closed.
#[tokio::test(flavor = "current_thread")]
async fn a_surplus_connection_is_accepted_and_closed() {
    let local = free_local_address();
    let stack = Stack::new();
    let acceptor = stack.bind(local).await.unwrap();
    let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address(), the_tester());

    let clients = async {
        let _first = TcpStream::connect(local).await.unwrap();
        let _reserve = TcpStream::connect(local).await.unwrap();
        let mut surplus = TcpStream::connect(local).await.unwrap();
        let mut byte = [0u8; 1];
        surplus.read(&mut byte).await.unwrap_or(0)
    };
    let entity = async {
        let mut buf = [0u8; 64];
        let event = entity.next_event(&mut buf, None).await;
        panic!("the entity reported {event:?}")
    };

    let read = timeout(PATIENCE, async {
        tokio::select! {
            read = clients => read,
            () = entity => unreachable!(),
        }
    })
    .await
    .unwrap();
    assert_eq!(read, 0, "the surplus connection was closed without a byte");
}
