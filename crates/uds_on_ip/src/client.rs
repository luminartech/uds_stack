//! The client side of `UDSonIP`: ISO 14229-5:2022 over one `DoIP` tester connection.

use core::future::Future;
use core::ops::Range;

use simple_doip::LogicalAddress;
use simple_doip::service::{self, ConnectionEvent, DoIpResult, Refusal, TesterConnection};
use uds_services::{AfterSend, ClientTransport, TransportEvent, UdsTransport};
use uds_session::{Address, Ai, Reloads, SResult, TaType, Timestamp};

use crate::error::ClientTransportError;
use crate::mapping::{
    ADDRESSES, PERIODIC_RESPONSE_PAYLOAD_TYPE, ai, awaits_response, from_logical, refused,
    response_pending, s_result, target_of, to_doip_ta_type, to_logical,
};

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
/// - **Reconnecting.** ISO 14229-5:2022 REQ 7.8 and REQ 7.10 have a client open a new
///   connection and activate routing again after the server closes it. A request made
///   while the connection is closed is accepted at once and waits; the next
///   [`UdsTransport::next_event`] reconnects for it through
///   [`TesterConnection::reconnect`], which gives the old connection up and waits out its
///   back-off first, and then sends it. That wait does not honour the call's deadline:
///   bound it by dropping the call, which leaves the request waiting, owed its confirm,
///   for the next event to reconnect for. A request withdrawn with `DoIP_TIMEOUT_A`
///   leaves the connection up and reconnects nothing.
/// - **A late reply.** A physical request that expects a response, to a server whose
///   last such request was confirmed and never answered, goes on a new connection, so
///   that answer, arriving after the client gave up on it, cannot be taken for this
///   one's. A response-pending message is not an answer. A request whose positive
///   response is suppressed expects none, so it neither leaves a reply to arrive late
///   nor gives up a connection a reply is awaited on.
/// - **Periodic responses.** A message of ISO 14229-5:2022 REQ 7.16's payload type
///   ([`PERIODIC_RESPONSE_PAYLOAD_TYPE`])
///   to this tester is a [`TransportEvent::Periodic`]; one truncated or too short to
///   carry an identifier is dropped.
///
/// # Servers
///
/// `PEERS` is how many servers behind the entity the transport tracks at once, for the
/// late-reply guard and for their closes: one for a sensor, and for a client of a gateway
/// as many as it has physical channels (`uds_services::uds_client`'s `physical`). A
/// server takes the place of one with nothing awaited and no prescribed close pending,
/// whose close would change nothing at the client. A physical request to a server when
/// every tracked one is busy is refused, confirmed failed, rather than sent untracked.
///
/// # Closes
///
/// A connection's end is one [`TransportEvent::Closed`] for each server tracked on it,
/// reported before anything from a new connection, and before any failed confirm the
/// transport owes. Its `expected` is true where the
/// server's last message was a positive `DiagnosticSessionControl` or `ECUReset`
/// response, after which REQ 7.9 and REQ 7.11 have a server close: the server's session
/// is then not the one the client knew. Any other end, a failure or a connection the
/// tester gave up, is not expected.
///
/// Functional addressing reaches the one entity the connection is to; fanning a request
/// out to several entities is not supported.
///
/// Time is the connection's: [`UdsTransport::now`] is
/// [`now`](simple_doip::service::DiagnosticConnection::now).
pub struct DoIpClientTransport<C, const QUEUE: usize, const PEERS: usize = 1> {
    connection: C,
    reloads: Reloads,
    max_data_size: Option<u32>,
    link: Link,
    /// The request the connection carries, awaiting its confirm, as it was made.
    sent: Option<Ai>,
    waiting: Waiting<QUEUE>,
    owed: Owed<PEERS>,
    peers: [Option<Peer>; PEERS],
}

/// The first octet of a positive `DiagnosticSessionControl` response, after which a
/// server leaving its software closes the connection (ISO 14229-5:2022 REQ 7.9).
const LEAVING_SESSION: u8 = 0x50;

/// The first octet of a positive `ECUReset` response, after which a server closes the
/// connection (ISO 14229-5:2022 REQ 7.11).
const RESETTING: u8 = 0x51;

/// Where a server's last request that expects an answer stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Awaiting {
    Nothing,
    Confirm,
    Answer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Up,
    Closed,
}

#[derive(Debug, Clone, Copy)]
struct Peer {
    address: Address,
    awaiting: Awaiting,
    /// Its last message was a positive response after which it may close the connection
    /// (ISO 14229-5:2022 REQ 7.9, REQ 7.11).
    leaving: bool,
}

impl Peer {
    /// Nothing is awaited from it and no prescribed close is pending, so its close would
    /// change nothing at the client.
    const fn idle(self) -> bool {
        matches!(self.awaiting, Awaiting::Nothing) && !self.leaving
    }
}

impl<C, const QUEUE: usize, const PEERS: usize> core::fmt::Debug
    for DoIpClientTransport<C, QUEUE, PEERS>
{
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

impl<C: TesterConnection, const QUEUE: usize, const PEERS: usize>
    DoIpClientTransport<C, QUEUE, PEERS>
{
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
    /// #   fn request(&mut self, _: LogicalAddress, _: TaType, _: &[u8])
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
            max_data_size: None,
            link: Link::Up,
            sent: None,
            waiting: Waiting::EMPTY,
            owed: Owed::EMPTY,
            peers: [None; PEERS],
        }
    }
}

impl<C, const QUEUE: usize, const PEERS: usize> DoIpClientTransport<C, QUEUE, PEERS> {
    /// The same transport, told the entity's *Max. data size* (ISO 13400-2:2019 Table 11),
    /// which [`UdsTransport::outbound_max`] then reports less the diagnostic message's
    /// addresses.
    ///
    /// # Arguments
    ///
    /// * `max_data_size` - the entity status response's *Max. data size*, as
    ///   `simple_doip`'s `tester::discovery::entity_status` returns it; `None` where the
    ///   entity reported none.
    #[must_use]
    pub const fn with_max_data_size(mut self, max_data_size: Option<u32>) -> Self {
        self.max_data_size = max_data_size;
        self
    }

    /// The connection, for inspection between events.
    #[must_use]
    pub const fn connection(&self) -> &C {
        &self.connection
    }

    fn peer(&mut self, address: Address) -> Option<&mut Peer> {
        self.peers
            .iter_mut()
            .flatten()
            .find(|p| p.address == address)
    }

    /// `address`'s slot, taking a free one, or one whose server's close would change
    /// nothing at the client; `None` where every server tracked is busy.
    fn track(&mut self, address: Address) -> Option<&mut Peer> {
        let slot = match self
            .peers
            .iter()
            .position(|p| p.is_some_and(|p| p.address == address))
        {
            Some(tracked) => tracked,
            None => self.peers.iter().position(|p| p.is_none_or(Peer::idle))?,
        };
        let peer = self.peers.get_mut(slot)?;
        if peer.is_none_or(|p| p.address != address) {
            *peer = Some(Peer {
                address,
                awaiting: Awaiting::Nothing,
                leaving: false,
            });
        }
        peer.as_mut()
    }

    /// Notes what `data`, from `sender`, answers and says of the connection's fate.
    fn heard(&mut self, sender: Address, data: &[u8]) {
        if let Some(peer) = self.peer(sender) {
            peer.leaving = matches!(data.first(), Some(&(LEAVING_SESSION | RESETTING)));
            if !response_pending(data) {
                peer.awaiting = Awaiting::Nothing;
            }
        }
    }

    /// Notes the confirm of `ai`'s request.
    fn confirmed(&mut self, ai: Ai, result: DoIpResult) {
        if result == DoIpResult::Ok {
            for peer in self.peers.iter_mut().flatten() {
                peer.leaving = false;
            }
        }
        if ai.ta_type != TaType::Physical {
            return;
        }
        if let Some(peer) = self.peers.iter_mut().flatten().find(|p| p.address == ai.ta)
            && peer.awaiting == Awaiting::Confirm
        {
            peer.awaiting = if result == DoIpResult::Ok {
                Awaiting::Answer
            } else {
                Awaiting::Nothing
            };
        }
    }

    /// Whether a request to `ai` that `expects` an answer must go on a new connection: its
    /// server's last request was confirmed and never answered, and that answer, arriving
    /// late, would be taken for this one's.
    fn late_reply_possible(&self, ai: Ai, expects: bool) -> bool {
        expects
            && ai.ta_type == TaType::Physical
            && self
                .peers
                .iter()
                .flatten()
                .any(|p| p.address == ai.ta && p.awaiting == Awaiting::Answer)
    }

    /// Notes a request to `ai` that `expects` an answer, now accepted.
    fn requested(&mut self, ai: Ai, expects: bool) {
        if expects
            && ai.ta_type == TaType::Physical
            && let Some(peer) = self.track(ai.ta)
        {
            peer.awaiting = Awaiting::Confirm;
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
            self.owed.close(peer.address, peer.leaving);
        }
        while let Some((ai, _)) = self.waiting.front() {
            self.owed.confirm(ai, s_result(DoIpResult::NoSocket));
            self.waiting.pop();
        }
    }

    /// Ends the connection on the transport's own account: no close it reports is the one
    /// a server prescribes.
    fn give_up(&mut self) {
        for peer in self.peers.iter_mut().flatten() {
            peer.leaving = false;
        }
        self.end();
    }
}

impl<C: TesterConnection, const QUEUE: usize, const PEERS: usize>
    DoIpClientTransport<C, QUEUE, PEERS>
{
    /// Owes `ai`'s failed confirm for `refusal`; `Err` where nothing more can be owed.
    fn refuse(&mut self, ai: Ai, refusal: Refusal) -> Result<(), ClientTransportError<C>> {
        if self.owed.may_refuse() {
            self.owed.confirm(ai, s_result(refused(refusal)));
            Ok(())
        } else {
            Err(ClientTransportError::Refused(refusal))
        }
    }

    /// Hands `data` to the connection, or holds it until the connection can take it,
    /// reconnected by the next [`UdsTransport::next_event`] where it has ended.
    fn send(&mut self, ai: Ai, data: &[u8]) -> Result<(), ClientTransportError<C>> {
        let ta = target_of(ai)?;
        let expects = awaits_response(data);
        if self.late_reply_possible(ai, expects) {
            self.give_up();
        }
        if ai.ta_type == TaType::Physical && self.track(ai.ta).is_none() {
            return self.refuse(ai, Refusal::NoRoom);
        }
        if self.link == Link::Closed || self.sent.is_some() || !self.waiting.is_empty() {
            return self.wait(ai, data, expects);
        }
        match self
            .connection
            .request(ta, to_doip_ta_type(ai.ta_type), data)
        {
            Ok(()) => {
                self.sent = Some(ai);
                self.requested(ai, expects);
                Ok(())
            }
            Err(Refusal::NoRoom) => self.wait(ai, data, expects),
            Err(Refusal::NotConnected) => {
                self.end();
                self.wait(ai, data, expects)
            }
            Err(refusal) => self.refuse(ai, refusal),
        }
    }

    fn wait(
        &mut self,
        ai: Ai,
        data: &[u8],
        expects: bool,
    ) -> Result<(), ClientTransportError<C>> {
        if self.waiting.push(ai, data) {
            self.requested(ai, expects);
            Ok(())
        } else {
            self.refuse(ai, Refusal::NoRoom)
        }
    }

    /// Reconnects where the connection has ended and a request waits for it. A reconnect
    /// that fails confirms every waiting request failed; one dropped part-way leaves them
    /// waiting for the next.
    async fn reconnect(&mut self) -> Result<(), ClientTransportError<C>> {
        if self.link == Link::Up || self.waiting.is_empty() {
            return Ok(());
        }
        if let Err(error) = self.connection.reconnect().await {
            while let Some((ai, _)) = self.waiting.front() {
                self.owed.confirm(ai, s_result(DoIpResult::NoSocket));
                self.waiting.pop();
            }
            return Err(ClientTransportError::Reconnect(error));
        }
        self.link = Link::Up;
        Ok(())
    }

    /// Hands the oldest waiting request to the connection, if it can take one.
    fn send_waiting(&mut self) {
        if self.sent.is_some() || self.link == Link::Closed {
            return;
        }
        let Some((ai, data)) = self.waiting.front_bytes() else {
            return;
        };
        match self
            .connection
            .request(to_logical(ai.ta), to_doip_ta_type(ai.ta_type), data)
        {
            Ok(()) => {
                self.sent = Some(ai);
                self.waiting.pop();
            }
            Err(Refusal::NoRoom) => {}
            Err(Refusal::NotConnected) => self.end(),
            Err(refusal) => {
                self.waiting.pop();
                self.owed.confirm(ai, s_result(refused(refusal)));
            }
        }
    }

    /// What `event` means here: an event to report, or `None` for one that is not.
    fn receive(
        &mut self,
        event: ConnectionEvent<'_>,
        start: usize,
    ) -> Result<Option<Received>, ClientTransportError<C>> {
        let own = self.connection.address().address();
        let span = |pdu: &[u8]| -> Result<Range<usize>, ClientTransportError<C>> {
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
            } if ta == own => {
                self.heard(from_logical(sa), pdu);
                Some(Received::Ind(ai(sa, ta, ta_type), span(pdu)?, None))
            }
            ConnectionEvent::IndicationTruncated {
                sa,
                ta,
                ta_type,
                pdu,
                length,
            } if ta == own => {
                self.heard(from_logical(sa), pdu);
                Some(Received::Ind(ai(sa, ta, ta_type), span(pdu)?, Some(length)))
            }
            ConnectionEvent::Unmodelled {
                payload_type: PERIODIC_RESPONSE_PAYLOAD_TYPE,
                data,
            } => match data {
                [sa_high, sa_low, ta_high, ta_low, pdid, record @ ..]
                    if LogicalAddress(u16::from_be_bytes([*ta_high, *ta_low])) == own =>
                {
                    let sa = LogicalAddress(u16::from_be_bytes([*sa_high, *sa_low]));
                    let ai = ai(sa, own, own.default_ta_type());
                    Some(Received::Periodic(ai, *pdid, span(record)?))
                }
                _ => None,
            },
            ConnectionEvent::Indication { .. }
            | ConnectionEvent::IndicationTruncated { .. }
            | ConnectionEvent::Unmodelled { .. }
            | ConnectionEvent::UnmodelledTruncated { .. } => None,
            ConnectionEvent::Confirm { result, .. } => self.sent.take().map(|ai| {
                self.confirmed(ai, result);
                Received::Event(Event::Conf(ai, s_result(result)))
            }),
            ConnectionEvent::Closed => {
                self.end();
                None
            }
            ConnectionEvent::Deadline => Some(Received::Event(Event::Deadline)),
        })
    }
}

/// What the connection reported, the PDU held as its place in the caller's buffer.
enum Received {
    Ind(Ai, Range<usize>, Option<usize>),
    Periodic(Ai, u8, Range<usize>),
    Event(Event),
}

impl<C: TesterConnection, const QUEUE: usize, const PEERS: usize> UdsTransport
    for DoIpClientTransport<C, QUEUE, PEERS>
{
    type Error = ClientTransportError<C>;

    /// The connection's
    /// [`MAX_PDU`](simple_doip::service::DiagnosticConnection::MAX_PDU),
    /// so `uds_services::uds_client` sizes no request past what the connection sends.
    const MAX_PDU: usize = C::MAX_PDU;

    /// `T_Data.req` as `DoIP_Data.request` (ISO 14229-5:2022 REQ 4.3 Table 4), from the
    /// address routing activation registered. Accepted or refused when called, waiting on
    /// nothing: where the last connection ended, the request waits for the next event to
    /// reconnect.
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
    fn t_data_req(
        &mut self,
        ai: Ai,
        data: &[u8],
        _after: AfterSend,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        core::future::ready(self.send(ai, data))
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
    /// closing, [`ClientTransportError::Reconnect`] where no new connection could be
    /// opened for a waiting request, which is then confirmed failed, and
    /// [`ClientTransportError::PduOutsideBuffer`] where the connection reports a PDU
    /// outside `buffer`.
    async fn next_event<'b>(
        &mut self,
        buffer: &'b mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent<'b>, Self::Error> {
        let start = buffer.as_ptr().addr();
        let found = loop {
            if let Some(owed) = self.owed.pop() {
                return Ok(owed.into());
            }
            self.reconnect().await?;
            self.send_waiting();
            if let Some(owed) = self.owed.pop() {
                return Ok(owed.into());
            }
            let event = self
                .connection
                .next_event(&mut *buffer, deadline.map(|at| service::Timestamp(at.0)))
                .await
                .map_err(ClientTransportError::Connection)?;
            match self.receive(event, start)? {
                Some(Received::Event(event)) => return Ok(event.into()),
                Some(found) => break found,
                None => {}
            }
        };
        let buffer: &'b [u8] = buffer;
        let data =
            |at: Range<usize>| buffer.get(at).ok_or(ClientTransportError::PduOutsideBuffer);
        Ok(match found {
            Received::Ind(ai, at, None) => TransportEvent::DataInd {
                ai,
                data: data(at)?,
            },
            Received::Ind(ai, at, Some(declared)) => TransportEvent::DataTooLong {
                ai,
                data: data(at)?,
                declared: Some(declared),
            },
            Received::Periodic(ai, pdid, at) => TransportEvent::Periodic {
                ai,
                pdid,
                data: data(at)?,
            },
            Received::Event(event) => event.into(),
        })
    }

    /// The *Max. data size* [`DoIpClientTransport::with_max_data_size`] was told, less
    /// the diagnostic message's addresses; `None` until then, because an entity reports
    /// it only over UDP, in its entity status response (ISO 13400-2:2019 Table 11).
    fn outbound_max(&self) -> Option<usize> {
        self.max_data_size.map(|max_data_size| {
            usize::try_from(max_data_size)
                .unwrap_or(usize::MAX)
                .saturating_sub(ADDRESSES)
        })
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

impl<C: TesterConnection, const QUEUE: usize, const PEERS: usize> ClientTransport
    for DoIpClientTransport<C, QUEUE, PEERS>
{
    /// [`TesterConnection::close`]. A request the connection carries is confirmed failed
    /// by it, and one waiting here by the transport; the close is reported by no
    /// [`TransportEvent::Closed`], even where the future is dropped before it finishes.
    ///
    /// # Errors
    ///
    /// [`ClientTransportError::Close`] where closing fails; the connection is closed
    /// either way.
    async fn close(&mut self) -> Result<(), Self::Error> {
        self.peers = [None; PEERS];
        self.end();
        self.connection
            .close()
            .await
            .map_err(ClientTransportError::Close)
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

/// How many refused requests' failed confirms the transport holds at once.
const REFUSALS: usize = 4;

/// How many failed confirms the transport holds: one for each waiting request a
/// connection's end fails, with [`REFUSALS`] beside them.
const CONFIRMS: usize = WAITING + REFUSALS;

/// The events owed: closes, each for a server tracked when its connection ended, then
/// failed confirms, oldest first.
///
/// Neither overflows. A connection ends at most once between reconnects, and the
/// transport reconnects only once nothing is owed, so at an end the closes are empty
/// and the confirms hold only refusals, which [`Owed::may_refuse`] keeps to
/// [`REFUSALS`].
#[derive(Debug)]
struct Owed<const PEERS: usize> {
    closes: [Option<(Address, bool)>; PEERS],
    confirms: [Option<(Ai, SResult)>; CONFIRMS],
}

impl<const PEERS: usize> Owed<PEERS> {
    const EMPTY: Self = Self {
        closes: [None; PEERS],
        confirms: [None; CONFIRMS],
    };

    fn close(&mut self, peer: Address, expected: bool) {
        let free = self.closes.iter_mut().find(|slot| slot.is_none());
        debug_assert!(free.is_some(), "a close owed past PEERS");
        if let Some(free) = free {
            *free = Some((peer, expected));
        }
    }

    fn confirm(&mut self, ai: Ai, result: SResult) {
        let free = self.confirms.iter_mut().find(|slot| slot.is_none());
        debug_assert!(free.is_some(), "a confirm owed past CONFIRMS");
        if let Some(free) = free {
            *free = Some((ai, result));
        }
    }

    fn may_refuse(&self) -> bool {
        self.confirms.iter().flatten().count() < REFUSALS
    }

    fn pop(&mut self) -> Option<Event> {
        if let Some((peer, expected)) = take_first(&mut self.closes) {
            return Some(Event::Closed { peer, expected });
        }
        take_first(&mut self.confirms).map(|(ai, result)| Event::Conf(ai, result))
    }
}

/// The oldest of `slots`, filled from the front, taken out.
fn take_first<T, const N: usize>(slots: &mut [Option<T>; N]) -> Option<T> {
    let taken = slots.first_mut()?.take()?;
    slots.rotate_left(1);
    Some(taken)
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
