//! The tester (client) role of the `DoIP` connection service, over `edge-nal`.
//!
//! [`Tester`] connects to a `DoIP` entity, activates routing, and is then a
//! [`DiagnosticConnection`]. It needs the `connection` feature; see the crate
//! documentation for what the integrator supplies.

#![deny(clippy::arithmetic_side_effects)]

use core::fmt;
use core::net::SocketAddr;

use edge_nal::{Readable, TcpConnect, TcpShutdown};
use embassy_time::{Duration, Instant, with_deadline};
use embedded_io_async::{Read, Write};

use crate::messages::{
    DiagnosticAckCode, DiagnosticMessage, Header, Message, NackCode, Payload, PayloadType,
    ProtocolVersion, RoutingActivationResponseCode,
};
use crate::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, NotATesterAddress,
};
use crate::wire::Decode;
use crate::{LogicalAddress, TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE, TaType};

mod confirm;
mod rx;
mod tx;

use rx::{Next, RxBuffer};
use tx::{Control, Outgoing, TooLarge};

/// How long the tester waits before repeating a routing activation request the entity
/// answered with confirmation required.
const ROUTING_CONFIRMATION_RETRY: Duration = Duration::from_secs(2);

/// `A_DoIP_Diagnostic_Message`'s timeout (ISO 13400-2:2019 Table 12).
const ACK_TIMEOUT: Duration =
    Duration::from_secs(TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE.as_secs());

/// The protocol version the tester sends.
const VERSION: ProtocolVersion = ProtocolVersion::V2019;

/// What a diagnostic message adds to its PDU: the generic header, and the source and
/// target addresses (ISO 13400-2:2019 Table 21).
pub const DIAGNOSTIC_MESSAGE_OVERHEAD: usize = Header::SIZE + 4;

/// The smallest `N` a [`Tester`] builds with: the longest routing activation response
/// (ISO 13400-2:2019 Table 48, with its OEM-specific field).
pub const MIN_N: usize = Header::SIZE + 13;

/// A logical address in the client range of ISO 13400-2:2019 Table 13, which a tester
/// may activate routing for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TesterAddress(LogicalAddress);

impl TesterAddress {
    /// Takes `address` as a tester's.
    ///
    /// # Arguments
    ///
    /// * `address` - within
    ///   [`LogicalAddress::MIN_CLIENT_ADDRESS`]..=[`LogicalAddress::MAX_CLIENT_ADDRESS`].
    ///
    /// # Errors
    ///
    /// [`NotATesterAddress`] for an `address` outside that range.
    pub fn new(address: LogicalAddress) -> Result<Self, NotATesterAddress> {
        if address.is_valid_client_address() {
            Ok(Self(address))
        } else {
            Err(NotATesterAddress { address })
        }
    }

    /// The address.
    #[must_use]
    pub const fn address(self) -> LogicalAddress {
        self.0
    }
}

impl From<TesterAddress> for LogicalAddress {
    fn from(address: TesterAddress) -> Self {
        address.0
    }
}

/// Why a tester could not connect and activate routing. Each leaves no connection open.
///
/// `E` is the socket's error, `edge_nal::TcpConnect::Error`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConnectError<E> {
    /// The socket failed connecting, reading or writing.
    #[error("socket failed: {0:?}")]
    Io(E),
    /// The entity refused routing activation with this response code
    /// (ISO 13400-2:2019 Table 49).
    ///
    /// Not retried: the only code the tester retries itself is
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`].
    ///
    /// [`RoutingSuccessfullyActivatedConfirmationRequired`]:
    ///     RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired
    #[error("routing activation denied: {0:?}")]
    RoutingActivationDenied(RoutingActivationResponseCode),
    /// The entity answered routing activation for a tester address other than this
    /// one's.
    #[error("routing activation answered for tester {0}")]
    ActivationAnsweredForAnotherTester(LogicalAddress),
    /// The entity rejected the routing activation request's header
    /// (ISO 13400-2:2019 Table 19).
    #[error("routing activation request rejected: generic header NACK {0:?}")]
    HeaderNack(NackCode),
    /// The entity closed the connection before answering routing activation.
    #[error("connection closed during routing activation")]
    Closed,
    /// The entity sent a message the tester cannot accept: a header out of sync, a
    /// protocol version the tester does not speak (ISO 13400-2:2019 Table 16), or a
    /// routing activation response of the wrong length (Table 48).
    #[error("invalid message during routing activation")]
    InvalidMessage,
}

/// Why a connected tester could not do what it was asked.
///
/// `E` is the socket's error, `edge_nal::TcpConnect::Error`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E> {
    /// The socket failed carrying data. The connection is closed, and the next
    /// [`DiagnosticConnection::next_event`] reports what follows from that.
    #[error("socket failed: {0:?}")]
    Io(E),
    /// A request's [`ConnectionEvent::Confirm`] has not been reported yet; this one was
    /// not accepted.
    #[error("a request is still awaiting its confirm")]
    RequestPending,
    /// The PDU is longer than [`Tester::MAX_PDU`]; it was not accepted.
    #[error("the PDU does not fit the tester's buffer")]
    MessageTooLarge,
    /// There is no connection: it closed, or a reconnect failed. Reconnect to continue.
    #[error("not connected")]
    NotConnected,
}

/// A `DoIP` tester: one TCP connection to one entity, with routing activated on it.
///
/// `N` is the longest `DoIP` message, generic header included, that the tester sends or
/// receives whole, and the size of each of its receive and transmit buffers. A
/// request's PDU is at most [`Tester::MAX_PDU`]; an indication's PDU longer than that
/// is delivered truncated. `N` is at least [`MIN_N`]: a smaller one fails to build,
/// though not to `cargo check`, which stops before the assertion is evaluated.
///
/// The stack is borrowed for `'s` because every socket it opens borrows it, and
/// [`Tester::reconnect`] opens another.
///
/// # Examples
///
/// A request, its confirm, then the entity's answer:
///
/// ```no_run
/// use simple_doip::service::{ConnectionEvent, DiagnosticConnection, DoIpResult};
/// use simple_doip::tester::{DIAGNOSTIC_MESSAGE_OVERHEAD, Tester, TesterAddress};
/// use simple_doip::{LogicalAddress, TCP_PORT, TaType};
/// # #[derive(Debug)]
/// # enum Failed {
/// #     Connect(simple_doip::tester::ConnectError<std::io::Error>),
/// #     Use(simple_doip::tester::Error<std::io::Error>),
/// # }
/// # impl From<simple_doip::tester::ConnectError<std::io::Error>> for Failed {
/// #     fn from(e: simple_doip::tester::ConnectError<std::io::Error>) -> Self { Self::Connect(e) }
/// # }
/// # impl From<simple_doip::tester::Error<std::io::Error>> for Failed {
/// #     fn from(e: simple_doip::tester::Error<std::io::Error>) -> Self { Self::Use(e) }
/// # }
///
/// # async fn example() -> Result<(), Failed> {
/// const N: usize = 4096 + DIAGNOSTIC_MESSAGE_OVERHEAD;
/// let stack = edge_nal_std::Stack::new();
/// let remote = ([192, 168, 0, 10], TCP_PORT).into();
/// let sa = TesterAddress::new(LogicalAddress(0x0E00)).expect("a tester address");
/// let mut tester = Tester::<_, N>::connect(&stack, remote, sa).await?;
///
/// tester.request(LogicalAddress(0x0001), TaType::Physical, &[0x3E, 0x00]).await?;
/// let mut buf = [0; Tester::<edge_nal_std::Stack, N>::MAX_PDU];
/// loop {
///     match tester.next_event(&mut buf, None).await? {
///         ConnectionEvent::Confirm { result: DoIpResult::Ok, .. } => {}
///         ConnectionEvent::Confirm { result, .. } => panic!("not delivered: {result:?}"),
///         ConnectionEvent::Indication { pdu, .. } => {
///             assert_eq!(pdu, [0x7E, 0x00]);
///             break;
///         }
///         ConnectionEvent::Closed => tester.reconnect().await?,
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct Tester<'s, C: TcpConnect + 's, const N: usize> {
    stack: &'s C,
    remote: SocketAddr,
    sa: TesterAddress,
    socket: Option<C::Socket<'s>>,
    rx: RxBuffer<N>,
    control: Control,
    outgoing: Outgoing<N>,
    exchange: Exchange,
    owed: Option<ConnectionEvent<'static>>,
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
    /// The longest PDU a request may carry, and the longest an indication delivers
    /// whole: `N` less [`DIAGNOSTIC_MESSAGE_OVERHEAD`].
    pub const MAX_PDU: usize = N.saturating_sub(DIAGNOSTIC_MESSAGE_OVERHEAD);

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
    /// * `sa` - this tester's source address.
    ///
    /// # Errors
    ///
    /// - [`ConnectError::Io`] where connecting, reading or writing fails.
    /// - [`ConnectError::RoutingActivationDenied`] for any response code but
    ///   [`RoutingActivationResponseCode::RoutingSuccessfullyActivated`] and
    ///   [`RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired`].
    /// - [`ConnectError::ActivationAnsweredForAnotherTester`],
    ///   [`ConnectError::HeaderNack`], [`ConnectError::Closed`] or
    ///   [`ConnectError::InvalidMessage`] where the entity answers otherwise than with a
    ///   response for `sa`.
    pub async fn connect(
        stack: &'s C,
        remote: SocketAddr,
        sa: TesterAddress,
    ) -> Result<Self, ConnectError<C::Error>> {
        const {
            assert!(N >= MIN_N, "a Tester's N must be at least MIN_N");
        };
        let mut tester = Self {
            stack,
            remote,
            sa,
            socket: None,
            rx: RxBuffer::new(),
            control: Control::new(),
            outgoing: Outgoing::new(),
            exchange: Exchange::default(),
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
    /// As for [`Tester::connect`]. After an error the tester is [`Error::NotConnected`]
    /// until a reconnect succeeds.
    pub async fn reconnect(&mut self) -> Result<(), ConnectError<C::Error>> {
        self.lose_connection(true).await;
        let result = self.establish().await;
        if result.is_err() {
            self.closed_reported = true;
        }
        result
    }

    /// Opens a new connection and activates routing on it, keeping it only on success.
    async fn establish(&mut self) -> Result<(), ConnectError<C::Error>> {
        self.rx.clear();
        self.control.clear();
        self.outgoing.clear();
        let stack = self.stack;
        let mut socket = stack.connect(self.remote).await.map_err(ConnectError::Io)?;
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
    ) -> Result<(), ConnectError<C::Error>> {
        let Self {
            sa,
            rx,
            control,
            outgoing,
            ..
        } = self;
        let sa = sa.address();
        loop {
            control.routing_activation_request(sa);
            let mut retry_at = None;
            loop {
                let flushed = flush(socket, control, outgoing, None)
                    .await
                    .map_err(ConnectError::Io)?;
                if flushed != Flush::Done {
                    return Err(ConnectError::Closed);
                }
                match rx.next() {
                    Err(_) => return Err(ConnectError::InvalidMessage),
                    Ok(Next::NeedMore) => {
                        match fill(socket, rx, retry_at).await.map_err(ConnectError::Io)? {
                            Fill::Data => {}
                            Fill::Eof => return Err(ConnectError::Closed),
                            Fill::TimedOut => break,
                        }
                    }
                    Ok(Next::Oversized { header, .. }) => {
                        if !spoken(&header) {
                            return Err(ConnectError::InvalidMessage);
                        }
                        rx.skip_oversized(&header);
                    }
                    Ok(Next::Frame(frame, consumed)) => {
                        let step =
                            on_activation_frame(&frame.header, frame.payload, sa, control)?;
                        rx.consume(consumed);
                        match step {
                            Activation::Waiting => {}
                            Activation::Activated => return Ok(()),
                            Activation::ConfirmationRequired => {
                                retry_at = Some(confirm::after(
                                    Instant::now(),
                                    ROUTING_CONFIRMATION_RETRY,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<C: TcpConnect, const N: usize> Tester<'_, C, N> {
    /// Gives up the connection, aborting it where the tester is the one ending it.
    ///
    /// An outstanding request is owed its confirm: `DoIP_NO_SOCKET` if its last byte
    /// never left, `DoIP_ERROR` if it left and was never acknowledged.
    async fn lose_connection(&mut self, abort: bool) {
        let socket = self.socket.take();
        if let Some(outstanding) = self.exchange.outstanding.take() {
            let result = if outstanding.sent {
                DoIpResult::Error
            } else {
                DoIpResult::NoSocket
            };
            self.owed = Some(outstanding.confirm(self.sa.address(), result));
        }
        self.exchange.late_ack_owed = false;
        self.rx.clear();
        self.control.clear();
        self.outgoing.clear();
        if let Some(mut socket) = socket
            && abort
        {
            socket.abort().await.ok();
        }
    }

    /// Owes the outstanding request `DoIP_TIMEOUT_A` once its time is up, and whether it
    /// did.
    ///
    /// The connection is given up where the request never finished being written, or
    /// where an earlier request's acknowledgement is still owed: the entity is then
    /// taking nothing in, or acknowledging nothing.
    async fn time_out(&mut self) -> bool {
        let Some(outstanding) = self.exchange.outstanding else {
            return false;
        };
        if outstanding.deadline > Instant::now() {
            return false;
        }
        self.exchange.outstanding = None;
        self.owed = Some(outstanding.confirm(self.sa.address(), DoIpResult::TimeoutA));
        if outstanding.sent && !self.exchange.late_ack_owed {
            self.exchange.late_ack_owed = true;
        } else {
            self.lose_connection(true).await;
        }
        true
    }
}

/// `DoIP_Data` over the tester's connection.
///
/// [`ConnectionEvent::Closed`] is reported once when the connection ends: the entity
/// closed it, the tester gave it up, or an [`Error::Io`] ended it. Every call after that
/// is [`Error::NotConnected`] until the tester reconnects.
impl<C: TcpConnect, const N: usize> DiagnosticConnection for Tester<'_, C, N> {
    type Error = Error<C::Error>;

    /// Queues `pdu` to `ta` as one diagnostic message, from the tester's source address,
    /// for [`DiagnosticConnection::next_event`] to write.
    ///
    /// Its [`ConnectionEvent::Confirm`] comes from the entity's acknowledgement
    /// (ISO 13400-2:2019 9.5): [`DoIpResult::Ok`] for a positive one, the result naming a
    /// negative one's code, or [`DoIpResult::TimeoutA`] where none arrives within
    /// `A_DoIP_Diagnostic_Message` (Table 12) of the request's last byte, or of the
    /// request where its bytes cannot all be written in that time. A physical request's
    /// acknowledgement must come from `ta`. An acknowledgement arriving after the
    /// timeout is discarded; a second timeout while one is still owed gives up the
    /// connection, as does a request that could not be written.
    ///
    /// # Cancel safety
    ///
    /// The future completes the first time it is polled, so the request is accepted
    /// exactly when it returns `Ok(())`. Dropped before that, nothing was queued.
    ///
    /// # Errors
    ///
    /// None of these is followed by a confirm:
    /// - [`Error::NotConnected`] once the connection has closed.
    /// - [`Error::RequestPending`] until an earlier request's confirm has been reported.
    /// - [`Error::MessageTooLarge`] for a `pdu` longer than [`Tester::MAX_PDU`].
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "accepting on the first poll is the contract"
    )]
    async fn request(
        &mut self,
        ta: LogicalAddress,
        ta_type: TaType,
        pdu: &[u8],
    ) -> Result<(), Self::Error> {
        if self.socket.is_none() {
            return Err(Error::NotConnected);
        }
        if self.exchange.outstanding.is_some() || self.owed.is_some() {
            return Err(Error::RequestPending);
        }
        let message = Message::diagnostic_message(VERSION, self.sa.address(), ta, pdu);
        self.outgoing
            .load(&message)
            .map_err(|TooLarge| Error::MessageTooLarge)?;
        self.exchange.outstanding = Some(Outstanding {
            ta,
            ta_type,
            deadline: confirm::after(Instant::now(), ACK_TIMEOUT),
            sent: false,
        });
        Ok(())
    }

    /// `embassy-time`'s clock, the one the tester's own timers run on.
    fn now(&self) -> u32 {
        confirm::millis(Instant::now())
    }

    /// The next event from the entity.
    ///
    /// Answers alive check requests itself and reports nothing for them. Reports
    /// [`ConnectionEvent::Unmodelled`] for a payload type ISO 13400-2:2019 Table 17
    /// reserves, its data truncated to `buf` and to `N` less the generic header. Ignores
    /// every other message a tester is not sent, every message in error (8.3.3), and a
    /// diagnostic message addressed to another tester. Gives up the connection on a
    /// header it cannot delimit or a protocol version other than ISO 13400-2:2012's or
    /// this edition's (Table 16), and after a negative acknowledgement on which the
    /// entity closes its socket: diagnostic `0x02` (REQ 7.DoIP-070), and generic header
    /// `0x00` and `0x04` (Table 19).
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe, provided the socket's reads, writes and readiness are; see the
    /// crate's `connection` feature documentation.
    async fn next_event<'b>(
        &mut self,
        buf: &'b mut [u8],
        deadline_ms: Option<u32>,
    ) -> Result<ConnectionEvent<'b>, Self::Error> {
        let until = deadline_ms
            .map(|deadline_ms| confirm::caller_deadline(deadline_ms, Instant::now()));
        let passed = || until.is_some_and(|until| until <= Instant::now());
        loop {
            if let Some(owed) = self.owed.take() {
                return Ok(owed);
            }
            if self.socket.is_none() {
                if self.closed_reported {
                    return Err(Error::NotConnected);
                }
                self.closed_reported = true;
                return Ok(ConnectionEvent::Closed);
            }
            if self.time_out().await {
                continue;
            }
            let wake = match (until, self.exchange.deadline()) {
                (Some(until), Some(ack)) => Some(until.min(ack)),
                (until, ack) => until.or(ack),
            };
            let Some(socket) = self.socket.as_mut() else {
                continue;
            };
            match flush(socket, &mut self.control, &mut self.outgoing, wake).await {
                Ok(Flush::Done) => self.exchange.mark_sent(),
                Ok(Flush::TimedOut) if passed() => return Ok(ConnectionEvent::Deadline),
                Ok(Flush::TimedOut) => continue,
                Ok(Flush::Closed) => {
                    self.lose_connection(false).await;
                    continue;
                }
                Err(error) => {
                    self.lose_connection(false).await;
                    return Err(Error::Io(error));
                }
            }
            let sa = self.sa.address();
            let reaction = match self.rx.next() {
                Err(_) => Reaction::Close { confirm: None },
                Ok(Next::NeedMore) => {
                    match fill(socket, &mut self.rx, wake).await {
                        Ok(Fill::TimedOut) if passed() => {
                            return Ok(ConnectionEvent::Deadline);
                        }
                        Ok(Fill::TimedOut | Fill::Data) => {}
                        Ok(Fill::Eof) => self.lose_connection(false).await,
                        Err(error) => {
                            self.lose_connection(false).await;
                            return Err(Error::Io(error));
                        }
                    }
                    continue;
                }
                Ok(Next::Oversized { header, head }) => {
                    let reaction = react(
                        &header,
                        head,
                        sa,
                        &mut self.exchange,
                        &mut self.control,
                        buf,
                    );
                    self.rx.skip_oversized(&header);
                    reaction
                }
                Ok(Next::Frame(frame, consumed)) => {
                    let reaction = react(
                        &frame.header,
                        frame.payload,
                        sa,
                        &mut self.exchange,
                        &mut self.control,
                        buf,
                    );
                    self.rx.consume(consumed);
                    reaction
                }
            };
            match reaction {
                Reaction::Ignore => {}
                Reaction::Deliver(delivered) => return Ok(delivered.into_event(buf)),
                Reaction::Close { confirm } => {
                    self.owed = confirm;
                    self.lose_connection(true).await;
                }
            }
        }
    }
}

/// The request awaiting its acknowledgement, and the one owed from before it.
#[derive(Debug, Default)]
struct Exchange {
    outstanding: Option<Outstanding>,
    /// A request was confirmed `DoIP_TIMEOUT_A` after its last byte was written, and the
    /// entity has not acknowledged it since.
    late_ack_owed: bool,
}

impl Exchange {
    fn deadline(&self) -> Option<Instant> {
        self.outstanding.map(|outstanding| outstanding.deadline)
    }

    /// Restarts the outstanding request's timer from its last byte, now written.
    fn mark_sent(&mut self) {
        if let Some(outstanding) = &mut self.outstanding
            && !outstanding.sent
        {
            outstanding.sent = true;
            outstanding.deadline = confirm::after(Instant::now(), ACK_TIMEOUT);
        }
    }

    /// Takes one acknowledgement from `source`, or a generic header NACK where `source`
    /// is `None`, as the next one the entity owes.
    fn acknowledge(
        &mut self,
        sa: LogicalAddress,
        source: Option<LogicalAddress>,
        result: DoIpResult,
    ) -> Option<ConnectionEvent<'static>> {
        if self.late_ack_owed {
            self.late_ack_owed = false;
            return None;
        }
        let outstanding = self
            .outstanding
            .take_if(|outstanding| outstanding.acknowledged_by(source))?;
        Some(outstanding.confirm(sa, result))
    }
}

#[derive(Debug, Clone, Copy)]
struct Outstanding {
    ta: LogicalAddress,
    ta_type: TaType,
    deadline: Instant,
    sent: bool,
}

impl Outstanding {
    fn acknowledged_by(&self, source: Option<LogicalAddress>) -> bool {
        self.ta_type != TaType::Physical || source.is_none_or(|source| source == self.ta)
    }

    fn confirm(self, sa: LogicalAddress, result: DoIpResult) -> ConnectionEvent<'static> {
        ConnectionEvent::Confirm {
            sa,
            ta: self.ta,
            ta_type: self.ta_type,
            result,
        }
    }
}

/// What a message from the entity calls for.
enum Reaction {
    Ignore,
    Deliver(Delivered),
    /// Give the connection up, reporting `confirm` first.
    Close {
        confirm: Option<ConnectionEvent<'static>>,
    },
}

/// An event whose data [`react`] has copied into the caller's buffer.
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

/// Whether the tester speaks `header`'s protocol version: ISO 13400-2:2012's or this
/// edition's (ISO 13400-2:2019 Table 16).
fn spoken(header: &Header) -> bool {
    matches!(
        header.protocol_version,
        ProtocolVersion::V2012 | ProtocolVersion::V2019
    )
}

/// Whether `header`'s payload length is one ISO 13400-2:2019 allows its payload type
/// (Tables 18, 21, 23, 25, 27 and 48).
fn sized_for_its_type(header: &Header) -> bool {
    let length = header.payload_length;
    match header.payload_type {
        PayloadType::NegativeAcknowledge => length == 1,
        PayloadType::RoutingActivationResponse => length == 9 || length == 13,
        PayloadType::AliveCheckRequest => length == 0,
        PayloadType::DiagnosticMessage
        | PayloadType::DiagnosticMessagePositiveAcknowledge
        | PayloadType::DiagnosticMessageNegativeAcknowledge => length >= 5,
        _ => true,
    }
}

/// Acts on one message from the entity, whose `payload` may be only the start of what
/// `header` describes, copying anything to report into `buf`.
fn react(
    header: &Header,
    payload: &[u8],
    sa: LogicalAddress,
    exchange: &mut Exchange,
    control: &mut Control,
    buf: &mut [u8],
) -> Reaction {
    if !spoken(header) {
        return Reaction::Close { confirm: None };
    }
    if !sized_for_its_type(header) {
        return Reaction::Ignore;
    }
    let deliver_confirm = |confirm: Option<ConnectionEvent<'static>>| match confirm {
        Some(confirm) => Reaction::Deliver(Delivered::Confirm(confirm)),
        None => Reaction::Ignore,
    };
    match (
        header.payload_type,
        Payload::decode(payload, header.payload_type),
    ) {
        (PayloadType::DiagnosticMessage, _) => {
            let Ok((message, _)) = DiagnosticMessage::decode(payload) else {
                return Reaction::Ignore;
            };
            let ta = message.target_address;
            if ta != sa && ta.is_valid_client_address() {
                return Reaction::Ignore;
            }
            let copied = message.user_data.len().min(buf.len());
            buf[..copied].copy_from_slice(&message.user_data[..copied]);
            Reaction::Deliver(Delivered::Indication {
                sa: message.source_address,
                ta,
                copied,
                length: (header.payload_length as usize).saturating_sub(4),
            })
        }
        (_, Ok(Payload::DiagnosticMessageAck(ack)))
            if ack.target_address == sa
                && ack.ack_code == DiagnosticAckCode::RoutingConfirmationAck =>
        {
            deliver_confirm(exchange.acknowledge(
                sa,
                Some(ack.source_address),
                DoIpResult::Ok,
            ))
        }
        (_, Ok(Payload::DiagnosticMessageNack(nack))) if nack.target_address == sa => {
            let result = confirm::from_diagnostic_nack(nack.nack_code);
            let confirm = exchange.acknowledge(sa, Some(nack.source_address), result);
            if result == DoIpResult::InvalidSa {
                Reaction::Close { confirm }
            } else {
                deliver_confirm(confirm)
            }
        }
        (_, Ok(Payload::DoIPNack(code))) => {
            let confirm = exchange.acknowledge(sa, None, confirm::from_header_nack(code));
            if matches!(
                code,
                NackCode::IncorrectPatternFormat | NackCode::InvalidPayloadLength
            ) {
                Reaction::Close { confirm }
            } else {
                deliver_confirm(confirm)
            }
        }
        (_, Ok(Payload::AliveCheckRequest)) => {
            control.alive_check_response(sa);
            Reaction::Ignore
        }
        (
            PayloadType::Reserved(payload_type)
            | PayloadType::ReservedVehicleManufacturer(payload_type),
            _,
        ) => {
            let copied = payload.len().min(buf.len());
            buf[..copied].copy_from_slice(&payload[..copied]);
            Reaction::Deliver(Delivered::Unmodelled {
                payload_type,
                copied,
            })
        }
        _ => Reaction::Ignore,
    }
}

/// Where routing activation stands after a frame.
enum Activation {
    Waiting,
    Activated,
    ConfirmationRequired,
}

fn on_activation_frame<E>(
    header: &Header,
    payload: &[u8],
    sa: LogicalAddress,
    control: &mut Control,
) -> Result<Activation, ConnectError<E>> {
    if !spoken(header) {
        return Err(ConnectError::InvalidMessage);
    }
    let sized = sized_for_its_type(header);
    match (
        header.payload_type,
        Payload::decode(payload, header.payload_type),
    ) {
        (
            PayloadType::RoutingActivationResponse,
            Ok(Payload::RoutingActivationResponse(response)),
        ) if sized => {
            if response.logical_address_tester != sa {
                return Err(ConnectError::ActivationAnsweredForAnotherTester(
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
                code => Err(ConnectError::RoutingActivationDenied(code)),
            }
        }
        (PayloadType::RoutingActivationResponse, _) => Err(ConnectError::InvalidMessage),
        (_, Ok(Payload::AliveCheckRequest)) if sized => {
            control.alive_check_response(sa);
            Ok(Activation::Waiting)
        }
        (_, Ok(Payload::DoIPNack(code))) if sized => Err(ConnectError::HeaderNack(code)),
        _ => Ok(Activation::Waiting),
    }
}

/// How a flush ended.
#[derive(Debug, PartialEq, Eq)]
enum Flush {
    Done,
    TimedOut,
    Closed,
}

/// Writes the control message, then the diagnostic message, keeping progress across a
/// cancel.
async fn flush<S: Write, const N: usize>(
    socket: &mut S,
    control: &mut Control,
    outgoing: &mut Outgoing<N>,
    until: Option<Instant>,
) -> Result<Flush, S::Error> {
    loop {
        let from_control = !control.pending().is_empty();
        let pending = if from_control {
            control.pending()
        } else {
            outgoing.pending()
        };
        if pending.is_empty() {
            return Ok(Flush::Done);
        }
        let write = socket.write(pending);
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
        if from_control {
            control.advance(written);
        } else {
            outgoing.advance(written);
        }
    }
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
