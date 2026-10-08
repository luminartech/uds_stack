//! A `DoIP` entity with no sockets, implementing [`DiagnosticEntity`] over a scripted
//! sequence of tester actions.
//!
//! Copied from `simple_doip/tests/entity_mock.rs` and extended with what a transport
//! test needs: a write that fails, a close that is cancelled once or fails, an unmodelled
//! payload, and a clock. An idle entity runs its clock to the caller's deadline, and
//! with no deadline has nothing left to do and says so.

use std::collections::VecDeque;

use uds_session::Timestamp;

use simple_doip::LogicalAddress;
use simple_doip::TaType;
use simple_doip::service::{ConnectionId, DiagnosticEntity, DoIpResult, EntityEvent};

pub const ENTITY: LogicalAddress = LogicalAddress(0x0001);
pub const TESTER: LogicalAddress = LogicalAddress(0x0E00);

/// A request that lives outside any buffer a caller lends the entity.
pub static STRAY: [u8; 2] = [0x3E, 0x00];

#[derive(Debug)]
pub enum Tester {
    Connects(LogicalAddress),
    Sends(LogicalAddress, Vec<u8>),
    Leaves(LogicalAddress),
    /// Sends a request the entity reports from memory other than the buffer it was
    /// lent ([`STRAY`]), breaking its contract.
    SendsOutsideBuffer(LogicalAddress),
}

/// Why the mock entity failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The script is spent and nothing is due: a run is over.
    Exhausted,
    /// A `close` the test asked to fail.
    CloseFailed,
    /// A `request` the test asked the entity to refuse.
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    Data(ConnectionId, Vec<u8>),
    Close(ConnectionId),
    /// A diagnostic message refused as too large (ISO 13400-2:2019 REQ 7.DoIP-072).
    TooLarge(ConnectionId),
}

#[derive(Debug)]
struct Slot {
    sa: LogicalAddress,
    outbound: VecDeque<(TaType, Vec<u8>)>,
    /// Whether an event has named the connection, which a `Closed` for it requires.
    named: bool,
}

#[derive(Debug)]
pub struct MockEntity<const CONNECTIONS: usize> {
    pub script: VecDeque<Tester>,
    table: [Option<Slot>; CONNECTIONS],
    confirms: VecDeque<(LogicalAddress, LogicalAddress, TaType, DoIpResult)>,
    pub wire: Vec<Wire>,
    /// Every PDU `request` accepted, whether or not a connection carried it.
    pub requested: Vec<Vec<u8>>,
    /// The next write fails with this result instead of reaching the wire.
    pub fail_next_write: Option<DoIpResult>,
    /// The next `close` yields once before acting, so a caller can drop it unfinished.
    pub close_yields: bool,
    /// The next `close` fails, after taking the connection out of the table as the
    /// contract requires.
    pub fail_next_close: bool,
    /// Tester actions are reported before pending confirmations rather than after.
    pub confirms_last: bool,
    /// The entity's clock, in milliseconds.
    pub clock: u32,
    /// Whether each `request` in turn is refused, as a full queue refuses it; once
    /// spent, every request is accepted.
    pub refuse: VecDeque<bool>,
    /// The longest request PDU the layer above accepts, once it has said.
    pub request_limit: Option<usize>,
}

impl<const CONNECTIONS: usize> MockEntity<CONNECTIONS> {
    pub fn new(script: impl IntoIterator<Item = Tester>) -> Self {
        Self {
            script: script.into_iter().collect(),
            table: core::array::from_fn(|_| None),
            confirms: VecDeque::new(),
            wire: Vec::new(),
            requested: Vec::new(),
            fail_next_write: None,
            close_yields: false,
            fail_next_close: false,
            confirms_last: false,
            clock: 0,
            refuse: VecDeque::new(),
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
        let Some(slot) = self.table[index].as_mut() else {
            return;
        };
        while let Some((ta_type, pdu)) = slot.outbound.pop_front() {
            let result = self.fail_next_write.take().unwrap_or_else(|| {
                self.wire.push(Wire::Data(Self::id(index), pdu));
                DoIpResult::Ok
            });
            self.confirms.push_back((ENTITY, slot.sa, ta_type, result));
        }
    }
}

impl<const CONNECTIONS: usize> MockEntity<CONNECTIONS> {
    fn confirm(&mut self) -> Option<EntityEvent<'static>> {
        let (sa, ta, ta_type, result) = self.confirms.pop_front()?;
        Some(EntityEvent::Confirm {
            sa,
            ta,
            ta_type,
            result,
        })
    }
}

/// Pending once, then ready.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if core::mem::replace(&mut self.0, true) {
            return core::task::Poll::Ready(());
        }
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "the mock has no sockets, so only a yielding close ever waits"
)]
impl<const CONNECTIONS: usize> DiagnosticEntity for MockEntity<CONNECTIONS> {
    type Error = Fault;
    const CONNECTIONS: usize = CONNECTIONS;
    const MAX_PDU: usize = usize::MAX;

    async fn request(
        &mut self,
        sa: LogicalAddress,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Self::Error> {
        if self.refuse.pop_front().unwrap_or(false) {
            return Err(Fault::Refused);
        }
        self.requested.push(pdu.to_vec());
        if sa != ENTITY {
            if let Some(index) = self.slot_of(ta) {
                self.flush(index);
            }
            self.confirms
                .push_back((sa, ta, ta_type, DoIpResult::UnknownSa));
            return Ok(());
        }
        match self.slot_of(ta) {
            Some(index) => self.table[index]
                .as_mut()
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
        self.clock
    }

    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline_ms: Option<u32>,
    ) -> Result<EntityEvent<'b>, Self::Error> {
        for index in 0..CONNECTIONS {
            self.flush(index);
        }
        if !self.confirms_last
            && let Some(confirm) = self.confirm()
        {
            return Ok(confirm);
        }
        while let Some(action) = self.script.pop_front() {
            match action {
                Tester::Connects(sa) => {
                    let free = self.table.iter().position(Option::is_none).unwrap();
                    self.table[free] = Some(Slot {
                        sa,
                        outbound: VecDeque::new(),
                        named: false,
                    });
                }
                Tester::Sends(sa, pdu) => {
                    let index = self.slot_of(sa).unwrap();
                    let connection = Self::id(index);
                    if self.request_limit.is_some_and(|limit| pdu.len() > limit) {
                        self.wire.push(Wire::TooLarge(connection));
                        continue;
                    }
                    self.table[index].as_mut().unwrap().named = true;
                    let fits = pdu.len().min(buf.len());
                    let delivered = &mut buf[..fits];
                    delivered.copy_from_slice(&pdu[..fits]);
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
                Tester::SendsOutsideBuffer(sa) => {
                    let index = self.slot_of(sa).unwrap();
                    self.table[index].as_mut().unwrap().named = true;
                    let connection = Self::id(index);
                    return Ok(EntityEvent::Indication {
                        connection,
                        sa,
                        ta: ENTITY,
                        ta_type: ENTITY.default_ta_type(),
                        pdu: &STRAY,
                    });
                }
                Tester::Leaves(sa) => {
                    let index = self.slot_of(sa).unwrap();
                    if self.table[index].take().unwrap().named {
                        return Ok(EntityEvent::Closed {
                            connection: Self::id(index),
                        });
                    }
                }
            }
        }
        if let Some(confirm) = self.confirm() {
            return Ok(confirm);
        }
        let deadline = deadline_ms.ok_or(Fault::Exhausted)?;
        let wait = Timestamp(self.clock).until(Timestamp(deadline));
        self.clock = self.clock.wrapping_add(wait);
        Ok(EntityEvent::Deadline)
    }

    async fn close(&mut self, connection: ConnectionId) -> Result<(), Self::Error> {
        if core::mem::take(&mut self.close_yields) {
            YieldOnce(false).await;
        }
        let index = connection.index();
        if self.table.get(index).is_some_and(Option::is_some) {
            self.flush(index);
            self.wire.push(Wire::Close(connection));
            self.table[index] = None;
        }
        if core::mem::take(&mut self.fail_next_close) {
            return Err(Fault::CloseFailed);
        }
        Ok(())
    }
}

/// Poll to completion with a no-op waker; every future here is ready within a bounded
/// number of polls.
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    let mut f = core::pin::pin!(f);
    for _ in 0..64 {
        if let core::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
    panic!("future did not complete in 64 polls");
}

/// Poll once, and drop the future whatever happened: what a driver does to the
/// `next_event` future that loses its race.
pub fn poll_once_and_drop<F: Future>(f: F) -> bool {
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    core::pin::pin!(f).poll(&mut cx).is_ready()
}
