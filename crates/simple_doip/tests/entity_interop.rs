//! `Entity` over `edge-nal-std`, driven by a tokio client framed by [`MessageCodec`]
//! rather than by this crate's `Tester`, which shares the entity's receive buffer: the
//! two cannot hide a framing bug they have in common.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;

use bytes::BytesMut;
use edge_nal::TcpBind;
use edge_nal_std::Stack;
use simple_doip::entity::{Entity, EntityAddress};
use simple_doip::message_codec::MessageCodec;
use simple_doip::messages::{
    ActivationTypeCode, DiagnosticAckCode, OwnedMessage, OwnedPayload, ProtocolVersion,
    RoutingActivationResponseCode,
};
use simple_doip::service::{DiagnosticEntity, DoIpResult, EntityConfig, EntityEvent};
use simple_doip::{LogicalAddress, TaType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use tokio_util::codec::{Decoder, Encoder};

const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const ENTITY: LogicalAddress = LogicalAddress(0x0001);

const PATIENCE: Duration = Duration::from_secs(5);

fn free_local_address() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

/// Answers one diagnostic message with its positive response, until it is confirmed.
async fn answer_one<E: DiagnosticEntity>(entity: &mut E) {
    loop {
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
                return;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Sends `messages`, then reads until the entity has sent a diagnostic message.
async fn exchange(local: SocketAddr, messages: &[OwnedMessage]) -> Vec<OwnedPayload> {
    let mut codec = MessageCodec::new();
    let mut stream = TcpStream::connect(local).await.unwrap();
    let mut out = BytesMut::new();
    for message in messages {
        codec.encode(message, &mut out).unwrap();
    }
    stream.write_all(&out).await.unwrap();

    let mut received = Vec::new();
    let mut inbound = BytesMut::new();
    loop {
        while let Some(message) = codec.decode(&mut inbound).unwrap() {
            let answered = matches!(message.payload, OwnedPayload::DiagnosticMessage(_));
            received.push(message.payload);
            if answered {
                return received;
            }
        }
        assert_ne!(
            stream.read_buf(&mut inbound).await.unwrap(),
            0,
            "closed early"
        );
    }
}

/// REQ 3.DoIP-131 and 7.DoIP-067, framed independently: an activation and a diagnostic
/// message written together are answered 0x10, acknowledged, and responded to.
#[tokio::test(flavor = "current_thread")]
async fn a_codec_framed_client_is_activated_acknowledged_and_answered() {
    let local = free_local_address();
    let stack = Stack::new();
    let acceptor = stack.bind(local).await.unwrap();
    let address = EntityAddress::new(ENTITY, LogicalAddress(0xE400)).unwrap();
    let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address, EntityConfig::default());
    let messages = [
        OwnedMessage::routing_activation_request(
            ProtocolVersion::V2019,
            TESTER,
            ActivationTypeCode::Default,
            None,
        ),
        OwnedMessage::diagnostic_message(
            ProtocolVersion::V2019,
            TESTER,
            ENTITY,
            vec![0x3E, 0x00],
        ),
    ];

    let ((), received) = timeout(PATIENCE, async {
        tokio::join!(answer_one(&mut entity), exchange(local, &messages))
    })
    .await
    .unwrap();

    let [
        OwnedPayload::RoutingActivationResponse(activation),
        OwnedPayload::DiagnosticMessageAck(ack),
        OwnedPayload::DiagnosticMessage(answer),
    ] = received.as_slice()
    else {
        panic!("unexpected {received:?}");
    };
    assert_eq!(
        activation.routing_activation_response_code,
        RoutingActivationResponseCode::RoutingSuccessfullyActivated
    );
    assert_eq!(activation.logical_address_tester, TESTER);
    assert_eq!(ack.ack_code, DiagnosticAckCode::RoutingConfirmationAck);
    assert_eq!((ack.source_address, ack.target_address), (ENTITY, TESTER));
    assert_eq!(
        (answer.source_address, answer.target_address),
        (ENTITY, TESTER)
    );
    assert_eq!(answer.user_data, [0x7E, 0x00]);
}
