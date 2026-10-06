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
use crate::service::{ConnectionEvent, DiagnosticConnection, NotATesterAddress};
use crate::wire::Decode;
use crate::{LogicalAddress, TaType};

mod confirm;
mod rx;
mod tx;

use rx::{Next, RxBuffer};
use tx::TxQueue;

/// How long the tester waits before repeating a routing activation request the entity
/// answered with confirmation required.
const ROUTING_CONFIRMATION_RETRY: Duration = Duration::from_secs(2);

/// The protocol version the tester sends.
const VERSION: ProtocolVersion = ProtocolVersion::V2019;

/// The smallest `N`: a routing activation request with an alive check response queued
/// beside it, which is also more than the longest routing activation response.
const MIN_N: usize = Header::SIZE + 7 + Header::SIZE + 2;

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
/// reconnecting opens another.
pub struct Tester<'s, C: TcpConnect + 's, const N: usize> {
    stack: &'s C,
    remote: SocketAddr,
    sa: LogicalAddress,
    socket: Option<C::Socket<'s>>,
    rx: RxBuffer<N>,
    tx: TxQueue<N>,
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
            closed_reported: true,
        };
        tester.establish().await?;
        Ok(tester)
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
    async fn lose_connection(&mut self, abort: bool) {
        if let Some(mut socket) = self.socket.take()
            && abort
        {
            socket.abort().await.ok();
        }
        self.rx.clear();
        self.tx.clear();
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

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "requests land in the next commit"
    )]
    async fn request(
        &mut self,
        _ta: LogicalAddress,
        _ta_type: TaType,
        _pdu: &[u8],
    ) -> Result<(), Self::Error> {
        Err(Error::NotConnected)
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
            let Some(socket) = self.socket.as_mut() else {
                if self.closed_reported {
                    return Err(Error::NotConnected);
                }
                self.closed_reported = true;
                return Ok(ConnectionEvent::Closed);
            };
            match flush(socket, &mut self.tx, until).await {
                Ok(Flush::Done) => {}
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
                    match fill(socket, &mut self.rx, until).await {
                        Ok(Fill::Data) => {}
                        Ok(Fill::TimedOut) => return Ok(ConnectionEvent::Deadline),
                        Ok(Fill::Eof) => self.lose_connection(false).await,
                        Err(error) => {
                            self.lose_connection(false).await;
                            return Err(Error::Io(error));
                        }
                    }
                    continue;
                }
                Ok(Next::Oversized { header, head }) => {
                    let delivered = on_frame(&header, head, self.sa, &mut self.tx, buf);
                    self.rx.skip_oversized(&header);
                    delivered
                }
                Ok(Next::Frame(frame, consumed)) => {
                    let delivered =
                        on_frame(&frame.header, frame.payload, self.sa, &mut self.tx, buf);
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

/// An event whose data [`on_frame`] has copied into the caller's buffer.
enum Delivered {
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
    buf: &mut [u8],
) -> Option<Delivered> {
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
