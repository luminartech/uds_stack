//! A tester finds the sensor's entity over UDP and connects to it: `simple_doip`'s
//! discovery at both ends of an `edge-nal-std` socket (ISO 13400-2:2019 7.4 to 7.6).
//!
//! The entity is asked at its own address rather than by broadcast, as a loopback test
//! cannot take `UDP_DISCOVERY`'s fixed port. Its announcements still go to the limited
//! broadcast address, where the host may refuse them; a refused one is given up.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test harness: a failed expectation is the test failing"
)]

use doip_loopback::{ENTITY, Loopback, SensorEntity, advance, on_loopback};
use simple_doip::entity::{Discovery, Entity, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{DiagnosticEntity, TesterConnection};
use simple_doip::tester::discovery::{self, Request};

const MAX_MESSAGE: usize = 4096;
const EID: [u8; 6] = [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF];

type Discovering = Entity<
    'static,
    edge_nal_std::TcpAcceptor,
    1,
    MAX_MESSAGE,
    1,
    Discovery<edge_nal_std::UdpSocket, FixedIdentity>,
>;

async fn entity(loopback: &Loopback) -> (Discovering, std::net::SocketAddr) {
    let (socket, at) = loopback.udp().await;
    let sensor: SensorEntity<MAX_MESSAGE> = loopback.entity();
    let identity = FixedIdentity::new(EID, DiagnosticPowerModeCode::Ready);
    (sensor.with_discovery(socket, identity, 0x5EED), at)
}

/// Runs the entity: it raises no event here, as no tester sends a diagnostic message.
async fn serve(mut entity: Discovering) -> ! {
    let mut buf = vec![0u8; MAX_MESSAGE];
    let event = entity.next_event(&mut buf, None).await;
    panic!("the entity raised {:?}", event.map(|_| ()));
}

/// Moves the mock clock 5 ms for each real millisecond, so the entity's announce wait
/// and the tester's `A_DoIP_Ctrl` run out.
async fn tick() -> ! {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        advance(embassy_time::Duration::from_millis(5));
    }
}

/// REQ 8.DoIP-046, 119 and 116: the tester identifies the entity, asks its status and
/// the vehicle's power mode, and connects to the address it answered from.
#[test]
fn a_tester_finds_the_entity_and_connects_to_it() {
    on_loopback(|loopback| async move {
        let (entity, entity_at) = entity(&loopback).await;
        let (mut socket, _) = loopback.udp().await;
        let script = async {
            let mut found = [None; 2];
            let kept =
                discovery::identify(&mut socket, entity_at, Request::All, &mut found)
                    .await
                    .expect("identified");
            assert_eq!(kept, 1);
            let [Some(sensor), None] = found else {
                panic!("expected the entity alone, got {found:?}");
            };
            assert_eq!(sensor.address(), entity_at);
            assert_eq!(sensor.identification().entity_id, EID);
            assert_eq!(sensor.identification().logical_address, ENTITY);

            let status = discovery::entity_status(&mut socket, sensor.address())
                .await
                .expect("a status");
            assert_eq!(status.max_data_size, Some(4088));
            assert_eq!(status.open_tcp_sockets, 0);
            let mode = discovery::power_mode(&mut socket, sensor.address())
                .await
                .expect("a power mode");
            assert_eq!(mode, DiagnosticPowerModeCode::Ready);

            assert_eq!(sensor.tcp_address().ip(), loopback.tcp_address().ip());
            let mut tester = loopback.tester().await.expect("activated");
            let status = discovery::entity_status(&mut socket, sensor.address())
                .await
                .expect("a status");
            assert_eq!(status.open_tcp_sockets, 1);
            tester.close().await.expect("closed");
        };
        tokio::select! {
            biased;
            () = script => {}
            never = serve(entity) => match never {},
            never = tick() => match never {},
        }
    });
}
