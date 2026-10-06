//! `Tester` over real loopback sockets, through `edge-nal-std`, against the tokio
//! `server::Server`: an independent implementation of the entity side, so a framing or
//! sequencing mistake the two would share in a mock cannot hide here.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use edge_nal_std::Stack;
use simple_doip::messages::{
    DiagnosticMessage, OwnedMessage, RoutingActivationRequest,
    RoutingActivationResponseCode,
};
use simple_doip::server::{ResponseWriter, Server, ServerConnectionHandler};
use simple_doip::service::{ConnectionEvent, DiagnosticConnection, DoIpResult};
use simple_doip::tester::{Error, Tester};
use simple_doip::{LogicalAddress, TaType};
use tokio::net::TcpListener;
use tokio::time::{Duration, timeout};

const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const ENTITY: LogicalAddress = LogicalAddress(0x0001);
const N: usize = 4108;

/// A real-time bound on each exchange. The tester's own timers run on `embassy-time`'s
/// mock clock here, which never advances, so a lost message would otherwise hang.
const PATIENCE: Duration = Duration::from_secs(5);

/// Answers routing activation with `code`, and every diagnostic message with an
/// acknowledgement followed by the message echoed back.
struct Echo {
    code: RoutingActivationResponseCode,
}

// `ResponseWriter` is the old server's trait object, which this test does not choose.
#[async_trait]
impl ServerConnectionHandler for Echo {
    fn get_vin(&self) -> [u8; 17] {
        [0; 17]
    }

    fn get_logical_address(&self) -> LogicalAddress {
        ENTITY
    }

    fn get_entity_id(&self) -> [u8; 6] {
        [0; 6]
    }

    fn get_group_id(&self) -> Option<[u8; 6]> {
        None
    }

    async fn routing_activation(
        &self,
        request: &RoutingActivationRequest,
    ) -> Result<OwnedMessage, simple_doip::Error> {
        Ok(OwnedMessage::routing_activation_response(
            self.protocol_version(),
            request.source_address,
            ENTITY,
            self.code,
            [0; 4],
            None,
        ))
    }

    async fn diagnostic_message(
        &self,
        message: &DiagnosticMessage<'_>,
        responses: &mut dyn ResponseWriter,
    ) -> Result<(), simple_doip::Error> {
        responses
            .send(OwnedMessage::diagnostic_message_ack(
                self.protocol_version(),
                message.target_address,
                message.source_address,
                message.user_data.to_vec(),
            ))
            .await?;
        responses
            .send(OwnedMessage::diagnostic_message(
                self.protocol_version(),
                message.target_address,
                message.source_address,
                message.user_data.to_vec(),
            ))
            .await
    }
}

/// Starts the tokio server on a free loopback port, answering activation with `code`.
async fn serve(code: RoutingActivationResponseCode) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Arc::new(Server::new(Echo { code }).unwrap());
    tokio::spawn(async move { server.run_server_with_listener(listener).await });
    address
}

/// ISO 13400-2:2019 12.5.2 and 9.5, end to end: routing activation, a diagnostic
/// message, its acknowledgement as the confirm, and the entity's answer as an
/// indication.
#[tokio::test(flavor = "multi_thread")]
async fn tester_and_tokio_server_exchange_a_request_over_loopback() {
    let remote = serve(RoutingActivationResponseCode::RoutingSuccessfullyActivated).await;
    let stack = Stack::new();
    let mut tester = timeout(PATIENCE, Tester::<_, N>::connect(&stack, remote, TESTER))
        .await
        .unwrap()
        .unwrap();
    let mut buf = [0; 64];

    timeout(
        PATIENCE,
        tester.request(ENTITY, TaType::Physical, &[0x3E, 0x00]),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(
        timeout(PATIENCE, tester.next_event(&mut buf, None))
            .await
            .unwrap()
            .unwrap(),
        ConnectionEvent::Confirm {
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            result: DoIpResult::Ok,
        }
    );
    assert_eq!(
        timeout(PATIENCE, tester.next_event(&mut buf, None))
            .await
            .unwrap()
            .unwrap(),
        ConnectionEvent::Indication {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            pdu: &[0x3E, 0x00],
        }
    );
}

/// ISO 13400-2:2019 Table 49: a denial from a real entity reaches the caller as its
/// response code.
#[tokio::test(flavor = "multi_thread")]
async fn the_tokio_server_can_deny_activation() {
    let remote = serve(RoutingActivationResponseCode::DeniedUnknownSourceAddress).await;
    let stack = Stack::new();

    let error = timeout(PATIENCE, Tester::<_, N>::connect(&stack, remote, TESTER))
        .await
        .unwrap()
        .unwrap_err();

    assert!(
        matches!(
            error,
            Error::RoutingActivationDenied(
                RoutingActivationResponseCode::DeniedUnknownSourceAddress
            )
        ),
        "{error:?}"
    );
}
