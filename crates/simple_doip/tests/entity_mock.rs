//! A `DoIP` entity with no sockets, implementing [`DiagnosticEntity`] over a scripted
//! sequence of tester actions, and the trait's contract pinned against it.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used, clippy::panic)]

use std::collections::VecDeque;

use simple_doip::LogicalAddress;
use simple_doip::TaType;
use simple_doip::service::{ConnectionId, DiagnosticEntity, DoIpResult, EntityEvent};

const ENTITY: LogicalAddress = LogicalAddress(0x0001);
const TESTER: LogicalAddress = LogicalAddress(0x0E00);

#[derive(Debug)]
enum Tester {
    Connects(LogicalAddress),
    Sends(LogicalAddress, Vec<u8>),
    Leaves(LogicalAddress),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Wire {
    Data(ConnectionId, Vec<u8>),
    Close(ConnectionId),
}

#[derive(Debug)]
struct Slot {
    sa: LogicalAddress,
    outbound: VecDeque<(TaType, Vec<u8>)>,
}

#[derive(Debug)]
struct MockEntity<const MCTS: usize> {
    script: VecDeque<Tester>,
    table: [Option<Slot>; MCTS],
    confirms: VecDeque<(LogicalAddress, LogicalAddress, TaType, DoIpResult)>,
    wire: Vec<Wire>,
    request_limit: Option<usize>,
}

impl<const MCTS: usize> MockEntity<MCTS> {
    fn new(script: impl IntoIterator<Item = Tester>) -> Self {
        Self {
            script: script.into_iter().collect(),
            table: core::array::from_fn(|_| None),
            confirms: VecDeque::new(),
            wire: Vec::new(),
            request_limit: None,
        }
    }

    fn id(index: usize) -> ConnectionId {
        ConnectionId::new(u8::try_from(index).unwrap())
    }

    fn slot_of(&self, sa: LogicalAddress) -> Option<usize> {
        self.table
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|slot| slot.sa == sa))
    }

    fn flush(&mut self, index: usize) {
        let Some(slot) = self.table.get_mut(index).and_then(Option::as_mut) else {
            return;
        };
        while let Some((ta_type, pdu)) = slot.outbound.pop_front() {
            self.wire.push(Wire::Data(Self::id(index), pdu));
            self.confirms
                .push_back((ENTITY, slot.sa, ta_type, DoIpResult::Ok));
        }
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "the mock has no sockets, so it never waits"
)]
impl<const MCTS: usize> DiagnosticEntity for MockEntity<MCTS> {
    type Error = core::convert::Infallible;
    const CONNECTIONS: usize = MCTS;
    const MAX_PDU: usize = usize::MAX;

    async fn request(
        &mut self,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Self::Error> {
        if sa != ENTITY {
            self.confirms
                .push_back((sa, ta, ta_type, DoIpResult::UnknownSa));
            return Ok(());
        }
        match self.slot_of(ta) {
            Some(index) => self
                .table
                .get_mut(index)
                .and_then(Option::as_mut)
                .unwrap()
                .outbound
                .push_back((ta_type, pdu.to_vec())),
            None => self
                .confirms
                .push_back((sa, ta, ta_type, DoIpResult::NoSocket)),
        }
        Ok(())
    }

    fn limit_requests(&mut self, max_pdu: usize) {
        self.request_limit = Some(max_pdu);
    }

    fn now(&self) -> u32 {
        0
    }

    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        _deadline_ms: Option<u32>,
    ) -> Result<EntityEvent<'b>, Self::Error> {
        for index in 0..MCTS {
            self.flush(index);
        }
        if let Some((sa, ta, ta_type, result)) = self.confirms.pop_front() {
            return Ok(EntityEvent::Confirm {
                sa,
                ta,
                ta_type,
                result,
            });
        }
        while let Some(action) = self.script.pop_front() {
            match action {
                Tester::Connects(sa) => {
                    let free = self.table.iter().position(Option::is_none).unwrap();
                    *self.table.get_mut(free).unwrap() = Some(Slot {
                        sa,
                        outbound: VecDeque::new(),
                    });
                }
                Tester::Sends(sa, pdu) => {
                    let connection = Self::id(self.slot_of(sa).unwrap());
                    let fits = pdu.len().min(buf.len());
                    let delivered = buf.get_mut(..fits).unwrap();
                    delivered.copy_from_slice(pdu.get(..fits).unwrap());
                    let (ta, ta_type) = (ENTITY, ENTITY.default_ta_type());
                    return Ok(if fits == pdu.len() {
                        EntityEvent::Indication {
                            connection,
                            sa,
                            ta,
                            ta_type,
                            pdu: delivered,
                        }
                    } else {
                        EntityEvent::IndicationTruncated {
                            connection,
                            sa,
                            ta,
                            ta_type,
                            pdu: delivered,
                            length: pdu.len(),
                        }
                    });
                }
                Tester::Leaves(sa) => {
                    let index = self.slot_of(sa).unwrap();
                    *self.table.get_mut(index).unwrap() = None;
                    return Ok(EntityEvent::Closed {
                        connection: Self::id(index),
                    });
                }
            }
        }
        Ok(EntityEvent::Deadline)
    }

    async fn close(&mut self, connection: ConnectionId) -> Result<(), Self::Error> {
        let index = connection.index();
        if self.table.get(index).is_some_and(Option::is_some) {
            self.flush(index);
            self.wire.push(Wire::Close(connection));
            *self.table.get_mut(index).unwrap() = None;
        }
        Ok(())
    }
}

/// What the layer above does with an entity, written against the trait alone: answer
/// each indication by echoing it to its sender, until the entity has nothing more.
async fn echo_until_idle<E: DiagnosticEntity>(entity: &mut E) -> Vec<EntityEvent<'static>> {
    let mut seen = Vec::new();
    loop {
        let mut buf = [0u8; 16];
        match entity.next_event(&mut buf, Some(0)).await.unwrap() {
            EntityEvent::Indication { sa, ta, pdu, .. } => {
                entity.request(ta, sa, TaType::Physical, pdu).await.unwrap();
            }
            EntityEvent::Deadline => return seen,
            EntityEvent::Confirm {
                sa,
                ta,
                ta_type,
                result,
            } => seen.push(EntityEvent::Confirm {
                sa,
                ta,
                ta_type,
                result,
            }),
            EntityEvent::Closed { connection } => {
                seen.push(EntityEvent::Closed { connection });
            }
            other @ EntityEvent::IndicationTruncated { .. } => {
                panic!("unexpected {other:?}")
            }
        }
    }
}

/// A request is routed by its target alone to the connection whose routing activation
/// registered that address (ISO 13400-2:2019 8.3.1), and is confirmed exactly once.
#[tokio::test]
async fn a_request_reaches_the_connection_its_target_activated_routing_on() {
    let other = LogicalAddress(0x0E80);
    let mut entity = MockEntity::<2>::new([
        Tester::Connects(other),
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x3E, 0x00]),
    ]);

    let seen = echo_until_idle(&mut entity).await;

    assert_eq!(
        entity.wire,
        [Wire::Data(ConnectionId::new(1), vec![0x3E, 0x00])]
    );
    assert_eq!(
        seen,
        [EntityEvent::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::Ok,
        }]
    );
}

/// A response to a tester that left between its request and the response is accepted
/// and confirmed with `DoIP_NO_SOCKET`, not refused: the trait's one-confirm
/// obligation covers a request no connection carried.
#[tokio::test]
async fn a_request_to_a_tester_that_has_left_is_confirmed_no_socket() {
    let mut entity = MockEntity::<1>::new([Tester::Connects(TESTER)]);
    let mut buf = [0u8; 16];
    assert_eq!(
        entity.next_event(&mut buf, Some(0)).await.unwrap(),
        EntityEvent::Deadline
    );
    entity.script.push_back(Tester::Leaves(TESTER));
    assert_eq!(
        entity.next_event(&mut buf, Some(0)).await.unwrap(),
        EntityEvent::Closed {
            connection: ConnectionId::new(0)
        }
    );

    entity
        .request(ENTITY, TESTER, TaType::Physical, &[0x7E, 0x00])
        .await
        .unwrap();

    assert_eq!(
        echo_until_idle(&mut entity).await,
        [EntityEvent::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::NoSocket,
        }]
    );
    assert_eq!(entity.wire, []);
}

/// The close a server makes after a positive `DiagnosticSessionControl` response
/// (ISO 14229-5:2022 REQ 7.9) is sent after the response, reports no `Closed`, and is
/// a no-op on a connection already gone.
#[tokio::test]
async fn the_prescribed_close_follows_the_response_and_reports_nothing() {
    let mut entity = MockEntity::<1>::new([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x10, 0x02]),
    ]);
    let mut buf = [0u8; 16];
    let EntityEvent::Indication { connection, sa, .. } =
        entity.next_event(&mut buf, None).await.unwrap()
    else {
        panic!("expected an indication");
    };

    entity
        .request(ENTITY, sa, TaType::Physical, &[0x50, 0x02])
        .await
        .unwrap();
    entity.close(connection).await.unwrap();
    entity.close(connection).await.unwrap();

    assert_eq!(
        entity.wire,
        [
            Wire::Data(connection, vec![0x50, 0x02]),
            Wire::Close(connection)
        ]
    );
    assert_eq!(
        echo_until_idle(&mut entity).await,
        [EntityEvent::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: TaType::Physical,
            result: DoIpResult::Ok,
        }]
    );
}

/// A tester that reconnects after the prescribed close arrives as a new connection,
/// which may reuse the closed one's id.
#[tokio::test]
async fn a_tester_returns_after_the_prescribed_close_as_a_new_connection() {
    let mut entity = MockEntity::<1>::new([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x11, 0x01]),
    ]);
    let mut buf = [0u8; 16];
    let EntityEvent::Indication { connection, .. } =
        entity.next_event(&mut buf, None).await.unwrap()
    else {
        panic!("expected an indication");
    };
    entity.close(connection).await.unwrap();
    entity
        .script
        .extend([Tester::Connects(TESTER), Tester::Sends(TESTER, vec![0x3E])]);

    let event = entity.next_event(&mut buf, None).await.unwrap();

    assert_eq!(
        event,
        EntityEvent::Indication {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: &[0x3E],
        }
    );
}

/// A message longer than the caller's buffer arrives truncated, with its whole length,
/// so the layer above can report the request too long rather than act on a fragment.
#[tokio::test]
async fn a_message_longer_than_the_buffer_is_indicated_truncated() {
    let mut entity = MockEntity::<1>::new([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x2E, 0xF1, 0x90, 0xAA, 0xBB]),
    ]);
    let mut buf = [0u8; 3];

    assert_eq!(
        entity.next_event(&mut buf, None).await.unwrap(),
        EntityEvent::IndicationTruncated {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: &[0x2E, 0xF1, 0x90],
            length: 5,
        }
    );
}

/// Closing an id the entity never issued names no connection, so it does nothing,
/// like closing one that has already gone.
#[tokio::test]
async fn closing_an_id_the_entity_never_issued_does_nothing() {
    let mut entity = MockEntity::<1>::new([Tester::Connects(TESTER)]);
    let mut buf = [0u8; 16];
    assert_eq!(
        entity.next_event(&mut buf, Some(0)).await.unwrap(),
        EntityEvent::Deadline
    );

    entity.close(ConnectionId::new(7)).await.unwrap();

    assert_eq!(entity.wire, []);
    entity.script.push_back(Tester::Sends(TESTER, vec![0x3E]));
    assert_eq!(
        entity.next_event(&mut buf, None).await.unwrap(),
        EntityEvent::Indication {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: TaType::Physical,
            pdu: &[0x3E],
        }
    );
}
