//! The tester (client) role of the `DoIP` connection service, over `edge-nal`.
//!
//! [`Tester`] connects to a `DoIP` entity, activates routing, and is then a
//! [`DiagnosticConnection`](crate::service::DiagnosticConnection). It needs the
//! `connection` feature; see the crate documentation for what the integrator supplies.

use core::fmt;
use core::net::SocketAddr;

use edge_nal::{Readable, TcpConnect, TcpShutdown};
use embassy_time::{Duration, Instant, with_deadline};
use embedded_io_async::{Read, Write};

use crate::RawFrame;
use crate::messages::{
    ActivationTypeCode, DiagnosticMessage, Header, Message, NackCode, Payload, PayloadType,
    ProtocolVersion, RoutingActivationResponseCode,
};
use crate::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, NotATesterAddress,
};
use crate::wire::{Decode, Encode};
use crate::{LogicalAddress, TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE, TaType};

mod confirm;
mod rx;
mod tx;

use rx::{Next, RxBuffer};
use tx::TxQueue;

/// How long the tester waits before repeating a routing activation request the entity
/// answered with confirmation required.
const ROUTING_CONFIRMATION_RETRY: Duration = Duration::from_secs(2);

/// `A_DoIP_Diagnostic_Message`'s timeout (ISO 13400-2:2019 Table 12): how long after a
/// request's last byte its acknowledgement may take before the request is lost.
const ACK_TIMEOUT: Duration =
    Duration::from_secs(TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE.as_secs());

/// An alive check response, for which the transmit queue keeps room beside a request.
const ALIVE_CHECK_RESPONSE: usize = Header::SIZE + 2;

/// The protocol version the tester sends.
const VERSION: ProtocolVersion = ProtocolVersion::V2019;

/// The smallest `N`: a routing activation request with an alive check response queued
/// beside it, which is also more than the longest routing activation response.
const MIN_N: usize = Header::SIZE + 7 + ALIVE_CHECK_RESPONSE;

/// Why a tester could not connect, or could not do what it was asked.
///
/// `E` is the socket's error, `edge_nal::TcpConnect::Error`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E: core::fmt::Debug> {
    /// The socket failed, connecting or carrying data. The connection is closed.
    #[error("socket failed: {0:?}")]
    Io(E),
    /// The source address the tester was given is outside the client range.
    #[error(transparent)]
    NotATesterAddress(#[from] NotATesterAddress),
    /// The entity refused routing activation with this response code
    /// (ISO 13400-2:2019 Table 49). The tester has closed the connection.
    ///
    /// Not retried: the only code the tester retries itself is
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`].
    ///
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`]:
    ///     RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired
    #[error("routing activation denied: {0:?}")]
    RoutingActivationDenied(RoutingActivationResponseCode),
    /// The entity answered routing activation for a tester address other than this
    /// one's. The tester has closed the connection.
    #[error("routing activation answered for tester {0}")]
    ActivationAnsweredForAnotherTester(LogicalAddress),
    /// The entity rejected the routing activation request's header
    /// (ISO 13400-2:2019 Table 19). The tester has closed the connection.
    #[error("routing activation request rejected: generic header NACK {0:?}")]
    HeaderNack(NackCode),
    /// The entity closed the connection before answering routing activation.
    #[error("connection closed during routing activation")]
    ClosedDuringActivation,
    /// A request is still awaiting its [`ConnectionEvent::Confirm`]; this one was not
    /// accepted.
    ///
    /// [`ConnectionEvent::Confirm`]: crate::service::ConnectionEvent::Confirm
    #[error("a request is still awaiting its confirm")]
    RequestPending,
    /// The PDU does not fit the tester's transmit queue; it was not accepted.
    #[error("the PDU does not fit the tester's buffer")]
    MessageTooLarge,
    /// The connection is closed and [`ConnectionEvent::Closed`] has been reported;
    /// reconnect to continue.
    ///
    /// [`ConnectionEvent::Closed`]: crate::service::ConnectionEvent::Closed
    #[error("not connected")]
    NotConnected,
}

/// A `DoIP` tester: one TCP connection to one entity, with routing activated on it.
///
/// `N` is the largest `DoIP` message, generic header included, that the tester holds in
/// each direction; it keeps one receive buffer and one transmit queue of `N` bytes. It
/// is at least the routing activation exchange's, which a smaller `N` fails to compile
/// for.
///
/// The stack is borrowed for `'s` because every socket it opens borrows it, and
/// [`Tester::reconnect`] opens another.
pub struct Tester<'s, C: TcpConnect + 's, const N: usize> {
    stack: &'s C,
    remote: SocketAddr,
    sa: LogicalAddress,
    socket: Option<C::Socket<'s>>,
    rx: RxBuffer<N>,
    tx: TxQueue<N>,
    outstanding: Option<Outstanding>,
    /// The confirm of a request whose connection was lost, reported before anything else.
    owed: Option<ConnectionEvent<'static>>,
    /// Whether [`ConnectionEvent::Closed`] has been reported for the connection that
    /// was lost.
    closed_reported: bool,
}

impl<C: TcpConnect, const N: usize> fmt::Debug for Tester<'_, C, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tester")
            .field("remote", &self.remote)
            .field("sa", &self.sa)
            .field("connected", &self.socket.is_some())
            .finish_non_exhaustive()
    }
}

impl<'s, C: TcpConnect, const N: usize> Tester<'s, C, N> {
    /// Connects to the entity at `remote` and activates routing for source address `sa`
    /// (ISO 13400-2:2019 12.5.2).
    ///
    /// Activation is the default type. An entity answering
    /// [`RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired`]
    /// is asked again on the same connection every two seconds until it answers
    /// otherwise, as ISO 13400-2:2019 REQ 3.DoIP-063 allows; alive checks are answered
    /// meanwhile.
    ///
    /// # Cancel safety
    ///
    /// Waits as long as the entity takes, with no timer of its own: ISO 13400-2 gives a
    /// tester none for routing activation. Bound it by dropping the future, for example
    /// with [`embassy_time::with_timeout`]; dropping it closes the connection.
    ///
    /// # Arguments
    ///
    /// * `stack` - the stack that opens this connection and any later one.
    /// * `remote` - the entity, usually on [`TCP_PORT`](crate::TCP_PORT).
    /// * `sa` - this tester's source address, within
    ///   [`LogicalAddress::MIN_CLIENT_ADDRESS`]..=[`LogicalAddress::MAX_CLIENT_ADDRESS`].
    ///
    /// # Errors
    ///
    /// - [`Error::NotATesterAddress`] for an `sa` outside the client range, before
    ///   connecting.
    /// - [`Error::Io`] where connecting, reading or writing fails.
    /// - [`Error::RoutingActivationDenied`] for any response code but
    ///   [`RoutingActivationResponseCode::RoutingSuccessfullyActivated`] and
    ///   [`RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired`].
    /// - [`Error::ActivationAnsweredForAnotherTester`],
    ///   [`Error::HeaderNack`] or [`Error::ClosedDuringActivation`] where the entity
    ///   answers otherwise than with a response for `sa`.
    ///
    /// Every error leaves no connection open.
    pub async fn connect(
        stack: &'s C,
        remote: SocketAddr,
        sa: LogicalAddress,
    ) -> Result<Self, Error<C::Error>> {
        const {
            assert!(
                N >= MIN_N,
                "a Tester's N must hold the routing activation exchange"
            );
        };
        if !sa.is_valid_client_address() {
            return Err(NotATesterAddress { address: sa }.into());
        }
        let mut tester = Self {
            stack,
            remote,
            sa,
            socket: None,
            rx: RxBuffer::new(),
            tx: TxQueue::new(),
            outstanding: None,
            owed: None,
            closed_reported: true,
        };
        tester.establish().await?;
        Ok(tester)
    }

    /// Gives up the connection, if there is one, then connects and activates routing
    /// again, as [`Tester::connect`] did.
    ///
    /// This is what ISO 14229-5:2022 REQ 7.8 and REQ 7.10 require of a client before it
    /// continues after the server closed the connection for a session change or a reset.
    /// A request still awaiting its confirm is confirmed with [`DoIpResult::Error`] or
    /// [`DoIpResult::NoSocket`] by the next
    /// [`next_event`](DiagnosticConnection::next_event), before anything from the new
    /// connection.
    ///
    /// # Cancel safety
    ///
    /// As for [`Tester::connect`]. Dropped, or failed, it leaves the tester with no
    /// connection; a dropped one reports [`ConnectionEvent::Closed`] for the old
    /// connection if that was not yet reported.
    ///
    /// # Errors
    ///
    /// As for [`Tester::connect`], except [`Error::NotATesterAddress`]. After an error
    /// the tester is [`Error::NotConnected`] until a reconnect succeeds.
    pub async fn reconnect(&mut self) -> Result<(), Error<C::Error>> {
        self.lose_connection(true).await;
        let result = self.establish().await;
        if result.is_err() {
            self.closed_reported = true;
        }
        result
    }

    /// Opens a new connection and activates routing on it, keeping it only on success.
    async fn establish(&mut self) -> Result<(), Error<C::Error>> {
        self.rx.clear();
        self.tx.clear();
        let stack = self.stack;
        let mut socket = stack.connect(self.remote).await.map_err(Error::Io)?;
        match self.activate(&mut socket).await {
            Ok(()) => {
                self.socket = Some(socket);
                self.closed_reported = false;
                Ok(())
            }
            Err(error) => {
                socket.abort().await.ok();
                Err(error)
            }
        }
    }

    async fn activate(
        &mut self,
        socket: &mut C::Socket<'s>,
    ) -> Result<(), Error<C::Error>> {
        let Self { sa, rx, tx, .. } = self;
        let request = Message::routing_activation_request(
            VERSION,
            *sa,
            ActivationTypeCode::Default,
            None,
        );
        loop {
            tx.push(&request).map_err(|_| Error::MessageTooLarge)?;
            let mut retry_at = None;
            loop {
                if flush(socket, tx, None).await.map_err(Error::Io)? != Flush::Done {
                    return Err(Error::ClosedDuringActivation);
                }
                match rx.next() {
                    Err(_) => return Err(Error::ClosedDuringActivation),
                    Ok(Next::NeedMore) => {
                        match fill(socket, rx, retry_at).await.map_err(Error::Io)? {
                            Fill::Data => {}
                            Fill::Eof => return Err(Error::ClosedDuringActivation),
                            Fill::TimedOut => break,
                        }
                    }
                    Ok(Next::Oversized { header, .. }) => rx.skip_oversized(&header),
                    Ok(Next::Frame(frame, consumed)) => {
                        let step = on_activation_frame(&frame, *sa, tx)?;
                        rx.consume(consumed);
                        match step {
                            Activation::Waiting => {}
                            Activation::Activated => return Ok(()),
                            Activation::ConfirmationRequired => {
                                retry_at =
                                    Some(Instant::now() + ROUTING_CONFIRMATION_RETRY);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<C: TcpConnect, const N: usize> Tester<'_, C, N> {
    /// Gives up the connection, aborting it first where the tester is the one ending it.
    ///
    /// An outstanding request is owed its confirm: `DoIP_NO_SOCKET` if its last byte
    /// never left, `DoIP_ERROR` if it left and was never acknowledged.
    async fn lose_connection(&mut self, abort: bool) {
        let socket = self.socket.take();
        if let Some(outstanding) = self.outstanding.take() {
            let result = if self.tx.written_through(outstanding.end) {
                DoIpResult::Error
            } else {
                DoIpResult::NoSocket
            };
            self.owed = Some(outstanding.confirm(self.sa, result));
        }
        self.rx.clear();
        self.tx.clear();
        if let Some(mut socket) = socket
            && abort
        {
            socket.abort().await.ok();
        }
    }

    /// The outstanding request's confirm with `DoIP_TIMEOUT_A`, if its time is up.
    fn timed_out(&mut self) -> Option<ConnectionEvent<'static>> {
        let deadline = self.outstanding?.ack_deadline?;
        if deadline > Instant::now() {
            return None;
        }
        let outstanding = self.outstanding.take()?;
        Some(outstanding.confirm(self.sa, DoIpResult::TimeoutA))
    }
}

/// `DoIP_Data` over the tester's connection.
///
/// [`ConnectionEvent::Closed`] is reported once when the connection ends: the entity
/// closed it, the tester closed it on a header it could not delimit, or an
/// [`Error::Io`] ended it. Every call after that is [`Error::NotConnected`] until the
/// tester reconnects.
impl<C: TcpConnect, const N: usize> DiagnosticConnection for Tester<'_, C, N> {
    type Error = Error<C::Error>;

    /// Sends `pdu` to `ta` as one diagnostic message, from the tester's source address.
    ///
    /// Its [`ConnectionEvent::Confirm`] comes from the entity's acknowledgement
    /// (ISO 13400-2:2019 9.5): [`DoIpResult::Ok`] for a positive one, the result naming a
    /// negative one's code, or [`DoIpResult::TimeoutA`] where none arrives within
    /// `A_DoIP_Diagnostic_Message` (Table 12) of the request's last byte.
    ///
    /// # Cancel safety
    ///
    /// The request is accepted once it is queued, which happens before the first await.
    /// Dropping the future after that leaves the rest to be written by
    /// [`DiagnosticConnection::next_event`], and the confirm still follows.
    ///
    /// # Errors
    ///
    /// None of these is followed by a confirm:
    /// - [`Error::NotConnected`] once the connection has closed.
    /// - [`Error::RequestPending`] while an earlier request awaits its confirm.
    /// - [`Error::MessageTooLarge`] for a message that does not fit beside the room the
    ///   tester keeps for an alive check response in `N`.
    /// - [`Error::Io`] where writing fails; the connection is then closed.
    async fn request(
        &mut self,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Self::Error> {
        if self.socket.is_none() {
            return Err(Error::NotConnected);
        }
        if self.outstanding.is_some() || self.owed.is_some() {
            return Err(Error::RequestPending);
        }
        let message = Message::diagnostic_message(VERSION, self.sa, ta, pdu);
        if message
            .encoded_size()
            .map_or(true, |size| size > N - ALIVE_CHECK_RESPONSE)
        {
            return Err(Error::MessageTooLarge);
        }
        let end = self.tx.push(&message).map_err(|_| Error::MessageTooLarge)?;
        self.outstanding = Some(Outstanding {
            ta,
            ta_type,
            end,
            ack_deadline: None,
        });
        let Some(socket) = self.socket.as_mut() else {
            return Ok(());
        };
        match flush(socket, &mut self.tx, None).await {
            Ok(_) => {
                start_ack_timer(&mut self.outstanding, &self.tx);
                Ok(())
            }
            Err(error) => {
                self.outstanding = None;
                self.lose_connection(false).await;
                Err(Error::Io(error))
            }
        }
    }

    /// `embassy-time`'s clock, the one the tester's own timers run on.
    fn now(&self) -> u32 {
        confirm::millis(Instant::now())
    }

    /// The next event from the entity.
    ///
    /// Answers alive check requests itself and reports nothing for them. Reports
    /// [`ConnectionEvent::Unmodelled`] for a payload type ISO 13400-2:2019 Table 17
    /// reserves, its data truncated to `buf`, and ignores every other message a tester
    /// is not sent.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe, provided the socket's reads and writes are; see the crate's
    /// `connection` feature documentation.
    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline_ms: Option<u32>,
    ) -> Result<ConnectionEvent<'b>, Self::Error> {
        let until = deadline_ms
            .map(|deadline_ms| confirm::caller_deadline(deadline_ms, Instant::now()));
        loop {
            if let Some(owed) = self.owed.take().or_else(|| self.timed_out()) {
                return Ok(owed);
            }
            let Some(socket) = self.socket.as_mut() else {
                if self.closed_reported {
                    return Err(Error::NotConnected);
                }
                self.closed_reported = true;
                return Ok(ConnectionEvent::Closed);
            };
            match flush(socket, &mut self.tx, until).await {
                Ok(Flush::Done) => start_ack_timer(&mut self.outstanding, &self.tx),
                Ok(Flush::TimedOut) => return Ok(ConnectionEvent::Deadline),
                Ok(Flush::Closed) => {
                    self.lose_connection(false).await;
                    continue;
                }
                Err(error) => {
                    self.lose_connection(false).await;
                    return Err(Error::Io(error));
                }
            }
            let delivered = match self.rx.next() {
                Err(_) => {
                    self.lose_connection(true).await;
                    continue;
                }
                Ok(Next::NeedMore) => {
                    let ack_deadline = self.outstanding.and_then(|o| o.ack_deadline);
                    let wake = match (until, ack_deadline) {
                        (Some(until), Some(ack)) => Some(until.min(ack)),
                        (until, ack) => until.or(ack),
                    };
                    match fill(socket, &mut self.rx, wake).await {
                        Ok(Fill::TimedOut) if wake == until => {
                            return Ok(ConnectionEvent::Deadline);
                        }
                        Ok(Fill::Data | Fill::TimedOut) => {}
                        Ok(Fill::Eof) => self.lose_connection(false).await,
                        Err(error) => {
                            self.lose_connection(false).await;
                            return Err(Error::Io(error));
                        }
                    }
                    continue;
                }
                Ok(Next::Oversized { header, head }) => {
                    let delivered = on_frame(
                        &header,
                        head,
                        self.sa,
                        &mut self.tx,
                        &mut self.outstanding,
                        buf,
                    );
                    self.rx.skip_oversized(&header);
                    delivered
                }
                Ok(Next::Frame(frame, consumed)) => {
                    let delivered = on_frame(
                        &frame.header,
                        frame.payload,
                        self.sa,
                        &mut self.tx,
                        &mut self.outstanding,
                        buf,
                    );
                    self.rx.consume(consumed);
                    delivered
                }
            };
            if let Some(delivered) = delivered {
                return Ok(delivered.into_event(buf));
            }
        }
    }
}

/// The request awaiting its acknowledgement.
#[derive(Debug, Clone, Copy)]
struct Outstanding {
    ta: LogicalAddress,
    ta_type: TaType,
    /// The transmit-stream position of the request's last byte.
    end: u64,
    /// When the request is lost, once that byte is written.
    ack_deadline: Option<Instant>,
}

impl Outstanding {
    fn confirm(self, sa: LogicalAddress, result: DoIpResult) -> ConnectionEvent<'static> {
        ConnectionEvent::Confirm {
            sa,
            ta: self.ta,
            ta_type: self.ta_type,
            result,
        }
    }
}

/// Starts the outstanding request's acknowledgement timer once its last byte is out.
fn start_ack_timer<const N: usize>(outstanding: &mut Option<Outstanding>, tx: &TxQueue<N>) {
    if let Some(outstanding) = outstanding
        && outstanding.ack_deadline.is_none()
        && tx.written_through(outstanding.end)
    {
        outstanding.ack_deadline = Some(Instant::now() + ACK_TIMEOUT);
    }
}

/// An event whose data [`on_frame`] has copied into the caller's buffer.
enum Delivered {
    Confirm(ConnectionEvent<'static>),
    Indication {
        sa: LogicalAddress,
        ta: LogicalAddress,
        copied: usize,
        length: usize,
    },
    Unmodelled {
        payload_type: u16,
        copied: usize,
    },
}

impl Delivered {
    fn into_event(self, buf: &mut [u8]) -> ConnectionEvent<'_> {
        match self {
            Self::Confirm(confirm) => confirm,
            Self::Indication {
                sa,
                ta,
                copied,
                length,
            } => {
                let pdu = &buf[..copied];
                let ta_type = ta.default_ta_type();
                if copied == length {
                    ConnectionEvent::Indication {
                        sa,
                        ta,
                        ta_type,
                        pdu,
                    }
                } else {
                    ConnectionEvent::IndicationTruncated {
                        sa,
                        ta,
                        ta_type,
                        pdu,
                        length,
                    }
                }
            }
            Self::Unmodelled {
                payload_type,
                copied,
            } => ConnectionEvent::Unmodelled {
                payload_type,
                data: &buf[..copied],
            },
        }
    }
}

/// Acts on one message from the entity, whose `payload` may be only the start of what
/// `header` describes, copying anything to report into `buf`.
fn on_frame<const N: usize>(
    header: &Header,
    payload: &[u8],
    sa: LogicalAddress,
    tx: &mut TxQueue<N>,
    outstanding: &mut Option<Outstanding>,
    buf: &mut [u8],
) -> Option<Delivered> {
    let mut acknowledge = |result: DoIpResult| {
        let acknowledged = outstanding.take_if(|o| o.ack_deadline.is_some())?;
        Some(Delivered::Confirm(acknowledged.confirm(sa, result)))
    };
    let copy = |data: &[u8], buf: &mut [u8]| {
        let copied = data.len().min(buf.len());
        buf[..copied].copy_from_slice(&data[..copied]);
        copied
    };
    match header.payload_type {
        PayloadType::DiagnosticMessage => {
            let (message, _) = DiagnosticMessage::decode(payload).ok()?;
            Some(Delivered::Indication {
                sa: message.source_address,
                ta: message.target_address,
                copied: copy(message.user_data, buf),
                length: (header.payload_length as usize).checked_sub(4)?,
            })
        }
        PayloadType::DiagnosticMessagePositiveAcknowledge => {
            match Payload::decode(payload, header.payload_type).ok()? {
                Payload::DiagnosticMessageAck(ack) if ack.target_address == sa => {
                    acknowledge(DoIpResult::Ok)
                }
                _ => None,
            }
        }
        PayloadType::DiagnosticMessageNegativeAcknowledge => {
            match Payload::decode(payload, header.payload_type).ok()? {
                Payload::DiagnosticMessageNack(nack) if nack.target_address == sa => {
                    acknowledge(confirm::from_diagnostic_nack(nack.nack_code))
                }
                _ => None,
            }
        }
        PayloadType::NegativeAcknowledge => {
            match Payload::decode(payload, header.payload_type) {
                Ok(Payload::DoIPNack(code)) => acknowledge(confirm::from_header_nack(code)),
                _ => None,
            }
        }
        PayloadType::AliveCheckRequest => {
            answer_alive_check(sa, tx);
            None
        }
        PayloadType::Reserved(payload_type)
        | PayloadType::ReservedVehicleManufacturer(payload_type) => {
            Some(Delivered::Unmodelled {
                payload_type,
                copied: copy(payload, buf),
            })
        }
        _ => None,
    }
}

/// Where routing activation stands after a frame.
enum Activation {
    Waiting,
    Activated,
    ConfirmationRequired,
}

fn on_activation_frame<E: fmt::Debug, const N: usize>(
    frame: &RawFrame<'_>,
    sa: LogicalAddress,
    tx: &mut TxQueue<N>,
) -> Result<Activation, Error<E>> {
    match Payload::decode(frame.payload, frame.header.payload_type) {
        Ok(Payload::RoutingActivationResponse(response)) => {
            if response.logical_address_tester != sa {
                return Err(Error::ActivationAnsweredForAnotherTester(
                    response.logical_address_tester,
                ));
            }
            match response.routing_activation_response_code {
                RoutingActivationResponseCode::RoutingSuccessfullyActivated => {
                    Ok(Activation::Activated)
                }
                RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired => {
                    Ok(Activation::ConfirmationRequired)
                }
                code => Err(Error::RoutingActivationDenied(code)),
            }
        }
        Ok(Payload::AliveCheckRequest) => {
            answer_alive_check(sa, tx);
            Ok(Activation::Waiting)
        }
        Ok(Payload::DoIPNack(code)) => Err(Error::HeaderNack(code)),
        _ => Ok(Activation::Waiting),
    }
}

/// Queues an alive check response. `N` leaves room for one beside any request, and the
/// queue is flushed before the next frame is read, so it always fits.
fn answer_alive_check<const N: usize>(sa: LogicalAddress, tx: &mut TxQueue<N>) {
    tx.push(&Message::alive_check_response(VERSION, sa)).ok();
}

/// How a flush ended.
#[derive(Debug, PartialEq, Eq)]
enum Flush {
    Done,
    TimedOut,
    Closed,
}

/// Writes everything queued, keeping its progress in `tx` across a cancel.
async fn flush<S: Write, const N: usize>(
    socket: &mut S,
    tx: &mut TxQueue<N>,
    until: Option<Instant>,
) -> Result<Flush, S::Error> {
    while !tx.pending().is_empty() {
        let write = socket.write(tx.pending());
        let written = match until {
            Some(until) => match with_deadline(until, write).await {
                Ok(written) => written?,
                Err(_) => return Ok(Flush::TimedOut),
            },
            None => write.await?,
        };
        if written == 0 {
            return Ok(Flush::Closed);
        }
        tx.advance(written);
    }
    Ok(Flush::Done)
}

/// How a read ended.
enum Fill {
    Data,
    Eof,
    TimedOut,
}

/// Waits until the socket is readable, then reads once into `rx`.
async fn fill<S: Read + Readable, const N: usize>(
    socket: &mut S,
    rx: &mut RxBuffer<N>,
    until: Option<Instant>,
) -> Result<Fill, S::Error> {
    match until {
        Some(until) => match with_deadline(until, socket.readable()).await {
            Ok(ready) => ready?,
            Err(_) => return Ok(Fill::TimedOut),
        },
        None => socket.readable().await?,
    }
    let read = socket.read(rx.free()).await?;
    if read == 0 {
        return Ok(Fill::Eof);
    }
    rx.filled(read);
    Ok(Fill::Data)
}
