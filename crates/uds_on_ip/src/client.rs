//! The client side of `UDSonIP`: ISO 14229-5:2022 over one `DoIP` tester connection.

use core::ops::Range;

use simple_doip::service::{self, ConnectionEvent, DoIpResult, Refusal, TesterConnection};
use uds_services::{AfterSend, ClientTransport, TransportEvent, UdsTransport};
use uds_session::{Address, Ai, Reloads, SResult, TaType, Timestamp};

use crate::error::ClientTransportError;
use crate::mapping::{ai, refused, s_result, target_of, to_doip_ta_type, to_logical};

/// ISO 14229-5:2022 over a `DoIP` tester connection: the client side of `UDSonIP`, for a
/// `uds_services::Client`.
///
/// One transport drives one [`TesterConnection`], to one entity, and every server reached
/// through it. It absorbs what the connection asks of its caller:
///
/// - **One request at a time.** A request made while the connection carries another
///   waits in the transport, in up to `QUEUE` bytes, and is sent once the one before it
///   is confirmed, so a keep-alive and a call's request never collide. `QUEUE` is at
///   least [`MAX_PDU`](simple_doip::service::DiagnosticConnection::MAX_PDU),
///   which [`Self::new`] checks at compile time, so a request always finds room behind a
///   keep-alive.
/// - **Refusals.** A request the connection refuses is accepted here all the same and
///   confirmed failed by the next [`UdsTransport::next_event`], as ISO 13400-2:2019 8.3.1
///   confirms every request.
/// - **Its own address.** A message to any address but the tester's is not this
///   tester's to take, and is dropped.
///
/// Functional addressing reaches the one entity the connection is to; fanning a request
/// out to several entities is not supported.
///
/// Time is the connection's: [`UdsTransport::now`] is
/// [`now`](simple_doip::service::DiagnosticConnection::now).
pub struct DoIpClientTransport<C, const QUEUE: usize> {
    connection: C,
    reloads: Reloads,
    link: Link,
    /// The request the connection carries, awaiting its confirm, as it was made.
    sent: Option<Ai>,
    waiting: Waiting<QUEUE>,
    owed: Owed,
    peers: [Option<Peer>; PEERS],
}

/// How many servers a transport remembers having addressed on one connection, each
/// told its own close.
const PEERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Up,
    Closed,
}

#[derive(Debug, Clone, Copy)]
struct Peer {
    address: Address,
}

impl<C, const QUEUE: usize> core::fmt::Debug for DoIpClientTransport<C, QUEUE> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpClientTransport")
            .field("connection", &"..")
            .field("reloads", &self.reloads)
            .field("link", &self.link)
            .field("sent", &self.sent)
            .field("peers", &self.peers)
            .finish_non_exhaustive()
    }
}

impl<C: TesterConnection, const QUEUE: usize> DoIpClientTransport<C, QUEUE> {
    /// A transport over `connection`, loading the session layer's response timer with
    /// `reloads`.
    ///
    /// Does not compile where `QUEUE` is below the connection's
    /// [`MAX_PDU`](simple_doip::service::DiagnosticConnection::MAX_PDU).
    ///
    /// # Arguments
    ///
    /// * `connection` - a connected [`TesterConnection`], routing activated.
    /// * `reloads` - the `tP6_Client` pair; see
    ///   [`profile::bench_reloads`](crate::profile::bench_reloads).
    ///
    /// # Examples
    ///
    /// A queue shorter than the longest request does not build:
    ///
    /// ```compile_fail
    /// # use simple_doip::service::{
    /// #     ConnectionEvent, DiagnosticConnection, Refusal, TesterAddress,
    /// #     TesterConnection, Timestamp,
    /// # };
    /// # use simple_doip::{LogicalAddress, TaType};
    /// # use uds_on_ip::{DoIpClientTransport, profile::bench_reloads};
    /// struct Tester;
    ///
    /// impl DiagnosticConnection for Tester {
    ///     const MAX_PDU: usize = 4084;
    ///     // ...
    /// #   type Error = ();
    /// #   async fn request(&mut self, _: LogicalAddress, _: TaType, _: &[u8])
    /// #       -> Result<(), Refusal> { Ok(()) }
    /// #   fn now(&self) -> Timestamp { Timestamp(0) }
    /// #   async fn next_event<'b>(&mut self, _: &'b mut [u8], _: Option<Timestamp>)
    /// #       -> Result<ConnectionEvent<'b>, ()> { Ok(ConnectionEvent::Deadline) }
    /// }
    /// # impl TesterConnection for Tester {
    /// #   type ReconnectError = ();
    /// #   type CloseError = ();
    /// #   type IoError = ();
    /// #   fn address(&self) -> TesterAddress {
    /// #       TesterAddress::new(LogicalAddress(0x0E00)).unwrap()
    /// #   }
    /// #   fn io_error(&self) -> Option<&()> { None }
    /// #   async fn reconnect(&mut self) -> Result<(), ()> { Ok(()) }
    /// #   async fn close(&mut self) -> Result<(), ()> { Ok(()) }
    /// # }
    ///
    /// let transport: DoIpClientTransport<Tester, 64> =
    ///     DoIpClientTransport::new(Tester, bench_reloads());
    /// ```
    #[must_use]
    pub const fn new(connection: C, reloads: Reloads) -> Self {
        const {
            assert!(
                QUEUE >= C::MAX_PDU,
                "QUEUE is below the connection's MAX_PDU"
            );
        };
        Self {
            connection,
            reloads,
            link: Link::Up,
            sent: None,
            waiting: Waiting::EMPTY,
            owed: Owed::EMPTY,
            peers: [None; PEERS],
        }
    }
}

impl<C, const QUEUE: usize> DoIpClientTransport<C, QUEUE> {
    /// The connection, for inspection between events.
    #[must_use]
    pub const fn connection(&self) -> &C {
        &self.connection
    }

    fn address(&mut self, peer: Address) {
        if self.peers.iter().flatten().any(|p| p.address == peer) {
            return;
        }
        if let Some(free) = self.peers.iter_mut().find(|p| p.is_none()) {
            *free = Some(Peer { address: peer });
        }
    }

    /// The connection ended: every server addressed on it is owed its close, and every
    /// waiting request its failed confirm.
    fn end(&mut self) {
        if self.link == Link::Closed {
            return;
        }
        self.link = Link::Closed;
        for peer in self.peers.iter_mut().filter_map(Option::take) {
            self.owed.push(Event::Closed {
                peer: peer.address,
                expected: false,
            });
        }
        while let Some((ai, _)) = self.waiting.front() {
            self.owed
                .push(Event::Conf(ai, s_result(DoIpResult::NoSocket)));
            self.waiting.pop();
        }
    }

    /// Owes `ai`'s failed confirm for `refusal`; `Err` where nothing more can be owed.
    fn refuse<E, R, X>(
        &mut self,
        ai: Ai,
        refusal: Refusal,
    ) -> Result<(), ClientTransportError<E, R, X>> {
        if self.owed.push(Event::Conf(ai, s_result(refused(refusal)))) {
            Ok(())
        } else {
            Err(ClientTransportError::Refused(refusal))
        }
    }
}

impl<C: TesterConnection, const QUEUE: usize> DoIpClientTransport<C, QUEUE> {
    /// Hands `data` to the connection, or holds it until the connection can take it.
    async fn send(&mut self, ai: Ai, data: &[u8]) -> Result<(), TransportError<C>> {
        let ta = target_of(ai)?;
        if self.sent.is_some() || !self.waiting.is_empty() {
            return self.wait(ai, data);
        }
        match self
            .connection
            .request(ta, to_doip_ta_type(ai.ta_type), data)
            .await
        {
            Ok(()) => {
                self.sent = Some(ai);
                Ok(())
            }
            Err(Refusal::NoRoom) => self.wait(ai, data),
            Err(Refusal::NotConnected) => {
                self.end();
                self.refuse(ai, Refusal::NotConnected)
            }
            Err(refusal) => self.refuse(ai, refusal),
        }
    }

    fn wait(&mut self, ai: Ai, data: &[u8]) -> Result<(), TransportError<C>> {
        if self.link == Link::Closed {
            return self.refuse(ai, Refusal::NotConnected);
        }
        if self.waiting.push(ai, data) {
            Ok(())
        } else {
            self.refuse(ai, Refusal::NoRoom)
        }
    }

    /// Hands the oldest waiting request to the connection, if it can take one.
    async fn send_waiting(&mut self) {
        if self.sent.is_some() || self.link == Link::Closed {
            return;
        }
        let Some((ai, data)) = self.waiting.front_bytes() else {
            return;
        };
        let request =
            self.connection
                .request(to_logical(ai.ta), to_doip_ta_type(ai.ta_type), data);
        match request.await {
            Ok(()) => {
                self.sent = Some(ai);
                self.waiting.pop();
            }
            Err(Refusal::NoRoom) => {}
            Err(Refusal::NotConnected) => self.end(),
            Err(refusal) => {
                self.waiting.pop();
                self.owed.push(Event::Conf(ai, s_result(refused(refusal))));
            }
        }
    }

    /// What `event` means here: an event to report, or `None` for one that is not.
    fn receive(
        &mut self,
        event: ConnectionEvent<'_>,
        start: usize,
    ) -> Result<Option<Received>, TransportError<C>> {
        let own = self.connection.address().address();
        let span = |pdu: &[u8]| -> Result<Range<usize>, TransportError<C>> {
            let at = pdu
                .as_ptr()
                .addr()
                .checked_sub(start)
                .ok_or(ClientTransportError::PduOutsideBuffer)?;
            Ok(at..at.saturating_add(pdu.len()))
        };
        Ok(match event {
            ConnectionEvent::Indication {
                sa,
                ta,
                ta_type,
                pdu,
            } if ta == own => Some(Received::Ind(ai(sa, ta, ta_type), span(pdu)?, None)),
            ConnectionEvent::IndicationTruncated {
                sa,
                ta,
                ta_type,
                pdu,
                length,
            } if ta == own => {
                Some(Received::Ind(ai(sa, ta, ta_type), span(pdu)?, Some(length)))
            }
            ConnectionEvent::Indication { .. }
            | ConnectionEvent::IndicationTruncated { .. }
            | ConnectionEvent::Unmodelled { .. }
            | ConnectionEvent::UnmodelledTruncated { .. } => None,
            ConnectionEvent::Confirm { result, .. } => self
                .sent
                .take()
                .map(|ai| Received::Event(Event::Conf(ai, s_result(result)))),
            ConnectionEvent::Closed => {
                self.end();
                None
            }
            ConnectionEvent::Deadline => Some(Received::Event(Event::Deadline)),
        })
    }
}

type TransportError<C> = ClientTransportError<
    <C as service::DiagnosticConnection>::Error,
    <C as TesterConnection>::ReconnectError,
    <C as TesterConnection>::CloseError,
>;

/// What the connection reported, the PDU held as its place in the caller's buffer.
enum Received {
    Ind(Ai, Range<usize>, Option<usize>),
    Event(Event),
}

impl<C: TesterConnection, const QUEUE: usize> UdsTransport
    for DoIpClientTransport<C, QUEUE>
{
    type Error = TransportError<C>;

    /// The connection's
    /// [`MAX_PDU`](simple_doip::service::DiagnosticConnection::MAX_PDU),
    /// so `uds_services::uds_client` sizes no request past what the connection sends.
    const MAX_PDU: usize = C::MAX_PDU;

    /// `T_Data.req` as `DoIP_Data.request` (ISO 14229-5:2022 REQ 4.3 Table 4), from the
    /// address routing activation registered.
    ///
    /// A request made while the connection carries another waits, and one the
    /// connection refuses is accepted all the same; either way the
    /// [`TransportEvent::DataConf`] that follows carries `ai` as given. A server never
    /// says what follows its message to a client, so `after` is ignored.
    ///
    /// # Errors
    ///
    /// [`ClientTransportError::Mapping`] if the addressing cannot be carried, and
    /// [`ClientTransportError::Refused`] if the connection refuses the request while the
    /// transport already owes as many confirmations as it can hold.
    async fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        _after: AfterSend,
    ) -> Result<(), Self::Error> {
        if ai.ta_type == TaType::Physical {
            self.address(ai.ta);
        }
        self.send(ai, data).await
    }

    /// The next `T_Data.ind` or `T_Data.conf`, a closed connection, or
    /// [`TransportEvent::Deadline`] when `deadline` passes first.
    ///
    /// A message longer than `buffer` is [`TransportEvent::DataTooLong`] with `declared`
    /// always `Some`. A connection's end is a [`TransportEvent::Closed`] for each server
    /// addressed on it, once; after that the call waits for `deadline`. Cancel-safe, as
    /// [`TesterConnection`]'s obligations make the connection's `next_event`.
    ///
    /// # Errors
    ///
    /// [`ClientTransportError::Connection`] where the connection fails other than by
    /// closing, and [`ClientTransportError::PduOutsideBuffer`] where it reports a PDU
    /// outside `buffer`.
    async fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, Self::Error> {
        let start = buffer.as_ptr().addr();
        let (ai, at, declared) = loop {
            if let Some(owed) = self.owed.pop() {
                return Ok(owed.into());
            }
            self.send_waiting().await;
            if let Some(owed) = self.owed.pop() {
                return Ok(owed.into());
            }
            let event = self
                .connection
                .next_event(&mut *buffer, deadline.map(|at| service::Timestamp(at.0)))
                .await
                .map_err(ClientTransportError::Connection)?;
            match self.receive(event, start)? {
                Some(Received::Ind(ai, at, declared)) => break (ai, at, declared),
                Some(Received::Event(event)) => return Ok(event.into()),
                None => {}
            }
        };
        let buffer: &'b [u8] = buffer;
        let data = buffer
            .get(at)
            .ok_or(ClientTransportError::PduOutsideBuffer)?;
        Ok(match declared {
            None => TransportEvent::DataInd { ai, data },
            Some(declared) => TransportEvent::DataTooLong {
                ai,
                data,
                declared: Some(declared),
            },
        })
    }

    /// `None`: an entity reports its *Max. data size* only over UDP.
    fn outbound_max(&self) -> Option<usize> {
        None
    }

    /// The `tP6_Client` reload pair the transport was built with: `DoIP` has no
    /// `T_DataSOM.ind`, so ISO 14229-2:2021 REQ 5.11 gives it `tP6` rather than `tP2`.
    fn channel_timing(&self) -> Reloads {
        self.reloads
    }

    /// [`now`](simple_doip::service::DiagnosticConnection::now).
    fn now(&self) -> Timestamp {
        Timestamp(self.connection.now().0)
    }
}

impl<C: TesterConnection, const QUEUE: usize> ClientTransport
    for DoIpClientTransport<C, QUEUE>
{
    /// [`TesterConnection::close`]. A request the connection carries is confirmed failed
    /// by it, and one waiting here by the transport; neither close is reported.
    ///
    /// # Errors
    ///
    /// [`ClientTransportError::Close`] where closing fails; the connection is closed
    /// either way.
    async fn close(&mut self) -> Result<(), Self::Error> {
        let closed = self.connection.close().await;
        self.peers = [None; PEERS];
        self.end();
        closed.map_err(ClientTransportError::Close)
    }
}

/// An event the transport reports without the connection's buffer.
#[derive(Debug, Clone, Copy)]
enum Event {
    Conf(Ai, SResult),
    Closed { peer: Address, expected: bool },
    Deadline,
}

impl From<Event> for TransportEvent<'_> {
    fn from(event: Event) -> Self {
        match event {
            Event::Conf(ai, result) => Self::DataConf { ai, result },
            Event::Closed { peer, expected } => Self::Closed { peer, expected },
            Event::Deadline => Self::Deadline,
        }
    }
}

/// How many events the transport can owe at once: a close for every server and a failed
/// confirm for every waiting request, with room for refusals beside them.
const OWED: usize = PEERS + WAITING + 4;

/// The events owed, oldest first.
#[derive(Debug)]
struct Owed([Option<Event>; OWED]);

impl Owed {
    const EMPTY: Self = Self([None; OWED]);

    fn push(&mut self, event: Event) -> bool {
        let Some(free) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *free = Some(event);
        true
    }

    fn pop(&mut self) -> Option<Event> {
        let event = self.0.first_mut()?.take()?;
        self.0.rotate_left(1);
        Some(event)
    }
}

/// How many requests can wait behind the one the connection carries: one per addressing
/// a client has outstanding at once, a call's request and its keep-alives.
const WAITING: usize = 4;

/// Requests waiting for the connection, oldest first, their bytes packed into `QUEUE`.
#[derive(Debug)]
struct Waiting<const QUEUE: usize> {
    bytes: [u8; QUEUE],
    entries: [Option<(Ai, usize)>; WAITING],
}

impl<const QUEUE: usize> Waiting<QUEUE> {
    const EMPTY: Self = Self {
        bytes: [0; QUEUE],
        entries: [None; WAITING],
    };

    fn is_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }

    fn used(&self) -> usize {
        self.entries.iter().flatten().map(|(_, len)| len).sum()
    }

    fn push(&mut self, ai: Ai, data: &[u8]) -> bool {
        let used = self.used();
        let Some(room) = self.bytes.get_mut(used..used.saturating_add(data.len())) else {
            return false;
        };
        let Some(free) = self.entries.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        room.copy_from_slice(data);
        *free = Some((ai, data.len()));
        true
    }

    fn front(&self) -> Option<(Ai, usize)> {
        self.entries.first().copied().flatten()
    }

    fn front_bytes(&self) -> Option<(Ai, &[u8])> {
        let (ai, len) = self.front()?;
        Some((ai, self.bytes.get(..len)?))
    }

    fn pop(&mut self) {
        let Some((_, len)) = self.front() else {
            return;
        };
        self.bytes.copy_within(len.., 0);
        if let Some(first) = self.entries.first_mut() {
            *first = None;
        }
        self.entries.rotate_left(1);
    }
}
