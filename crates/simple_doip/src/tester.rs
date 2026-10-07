//! The tester (client) role of the `DoIP` connection service, over `edge-nal`.
//!
//! [`Tester`] connects to a `DoIP` entity, activates routing, and is then a
//! [`DiagnosticConnection`]. It needs the `connection` feature; see the crate
//! documentation for what the integrator supplies.

#![deny(clippy::arithmetic_side_effects)]

use core::fmt;
use core::net::SocketAddr;

use edge_nal::{Close, Readable, TcpConnect, TcpShutdown};
use embassy_time::{Duration, Instant, Timer, with_deadline};
use embedded_io_async::{Read, Write};

use crate::messages::{
    DiagnosticAckCode, DiagnosticMessage, Header, Message, NackCode, Payload, PayloadType,
    ProtocolVersion, RoutingActivationResponseCode,
};
use crate::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, TesterAddress, TesterConnection,
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

/// How long [`TesterConnection::reconnect`] waits after a connection is lost before it
/// connects again: an entity may hold the tester's address for a couple of seconds after
/// the socket closes and refuse its routing activation meanwhile (`0x03`, ISO
/// 13400-2:2019 Table 49). Three seconds is what a sensor that does so was measured to
/// need.
pub const RECONNECT_BACKOFF: Duration = Duration::from_secs(3);

/// The protocol version the tester sends.
const VERSION: ProtocolVersion = ProtocolVersion::V2019;

/// What a diagnostic message adds to its PDU: the generic header, and the source and
/// target addresses (ISO 13400-2:2019 Table 21).
pub const DIAGNOSTIC_MESSAGE_OVERHEAD: usize = Header::SIZE + 4;

/// The smallest `N` a [`Tester`] builds with: the longest routing activation response
/// (ISO 13400-2:2019 Table 48, with its OEM-specific field).
pub const MIN_N: usize = Header::SIZE + 13;

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

/// Why a tester did not accept a request. None is followed by a confirm.
///
/// A connection ending is never one of these: it is [`ConnectionEvent::Closed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A request's [`ConnectionEvent::Confirm`] has not been reported yet; this one was
    /// not accepted.
    #[error("a request is still awaiting its confirm")]
    RequestPending,
    /// The PDU is longer than [`MAX_PDU`](DiagnosticConnection::MAX_PDU); it was not
    /// accepted.
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
/// request's PDU is at most [`MAX_PDU`](DiagnosticConnection::MAX_PDU); an indication's
/// PDU longer than that is delivered truncated. `N` is at least [`MIN_N`]: a smaller one
/// fails to build, though not to `cargo check`, which stops before the assertion is
/// evaluated.
///
/// The tester sends ISO 13400-2:2019's protocol version, so an entity that speaks only
/// an earlier edition refuses it.
///
/// The stack is borrowed for `'s` because every socket it opens borrows it, and
/// [`TesterConnection::reconnect`] opens another.
///
/// # What the caller owes
///
/// - **One request at a time.** [`DiagnosticConnection::request`] is
///   [`Error::RequestPending`] until the previous request's
///   [`ConnectionEvent::Confirm`] has been reported, so a caller that wants to send
///   another meanwhile, such as a keep-alive or a second channel's request, queues it.
/// - **[`DiagnosticConnection::next_event`] kept polled.** The tester does its I/O only
///   while it, [`Tester::connect`] or [`TesterConnection::reconnect`] is being polled:
///   nothing is written and no alive check is answered otherwise. An entity checks
///   that a tester is alive within `T_TCP_Alive_Check` (ISO 13400-2:2019 Table 12)
///   before giving its socket to another tester, so an idle caller loses the
///   connection. While a request is being written, nothing is read.
/// - **A new connection after a lost request.** A request not acknowledged within
///   `A_DoIP_Diagnostic_Message` (Table 12, 2 s) is confirmed
///   [`DoIpResult::TimeoutA`], and once any of it was written the connection is given
///   up and [`ConnectionEvent::Closed`] follows. Table 12 says the request or the
///   response "shall be considered lost"; keeping the connection would let the late
///   acknowledgement or response be taken for a later request's, which a tester that
///   carries one request at a time cannot tell apart.
/// - **[`TesterConnection::close`] to end it.** It closes the connection gracefully;
///   dropping the tester leaves that to the backend.
///
/// # Examples
///
/// A request, its confirm, then the entity's answer. A request that is lost or whose
/// connection ends is repeated, on a new connection where the old one is gone:
///
/// ```no_run
/// use simple_doip::service::{
///     ConnectionEvent, DiagnosticConnection, DoIpResult, TesterConnection,
/// };
/// use simple_doip::service::TesterAddress;
/// use simple_doip::tester::{DIAGNOSTIC_MESSAGE_OVERHEAD, Error, Tester};
/// use simple_doip::{LogicalAddress, TCP_PORT, TaType};
/// # #[derive(Debug)]
/// # enum Failed {
/// #     Connect(simple_doip::tester::ConnectError<std::io::Error>),
/// #     Use(Error),
/// #     Refused(DoIpResult),
/// #     Close(std::io::Error),
/// # }
/// # impl From<simple_doip::tester::ConnectError<std::io::Error>> for Failed {
/// #     fn from(e: simple_doip::tester::ConnectError<std::io::Error>) -> Self { Self::Connect(e) }
/// # }
/// # impl From<Error> for Failed {
/// #     fn from(e: Error) -> Self { Self::Use(e) }
/// # }
/// # impl From<std::io::Error> for Failed {
/// #     fn from(e: std::io::Error) -> Self { Self::Close(e) }
/// # }
///
/// # async fn example() -> Result<(), Failed> {
/// const N: usize = 4096 + DIAGNOSTIC_MESSAGE_OVERHEAD;
/// let stack = edge_nal_std::Stack::new();
/// let remote = ([192, 168, 0, 10], TCP_PORT).into();
/// let sa = TesterAddress::new(LogicalAddress(0x0E00)).expect("a tester address");
/// let mut tester = Tester::<_, N>::connect(&stack, remote, sa).await?;
///
/// let mut buf = [0; Tester::<edge_nal_std::Stack, N>::MAX_PDU];
/// 'send: loop {
///     match tester.request(LogicalAddress(0x0001), TaType::Physical, &[0x3E, 0x00]).await {
///         Err(Error::NotConnected) => {
///             tester.reconnect().await?;
///             continue 'send;
///         }
///         accepted => accepted?,
///     }
///     loop {
///         match tester.next_event(&mut buf, None).await? {
///             ConnectionEvent::Confirm { result: DoIpResult::Ok, .. } => {}
///             ConnectionEvent::Confirm {
///                 result: DoIpResult::TimeoutA | DoIpResult::NoSocket | DoIpResult::Error,
///                 ..
///             }
///             | ConnectionEvent::Closed => continue 'send,
///             ConnectionEvent::Confirm { result, .. } => return Err(Failed::Refused(result)),
///             ConnectionEvent::Indication { pdu, .. } => {
///                 assert_eq!(pdu, [0x7E, 0x00]);
///                 break 'send;
///             }
///             _ => {}
///         }
///     }
/// }
/// tester.close().await?;
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
    io_error: Option<C::Error>,
    lost_at: Option<Instant>,
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
            io_error: None,
            lost_at: None,
        };
        tester.establish().await?;
        Ok(tester)
    }

    /// The socket error that ended the last connection, if one did, until a new
    /// connection is established.
    ///
    /// [`DiagnosticConnection::next_event`] reports such an end as
    /// [`ConnectionEvent::Closed`], never as an error, so this is where its cause is
    /// kept.
    #[must_use]
    pub fn io_error(&self) -> Option<&C::Error> {
        self.io_error.as_ref()
    }

    /// Opens a new connection and activates routing on it, keeping it only on success.
    async fn establish(&mut self) -> Result<(), ConnectError<C::Error>> {
        let stack = self.stack;
        let mut socket = stack.connect(self.remote).await.map_err(ConnectError::Io)?;
        match self.activate(&mut socket).await {
            Ok(()) => {
                self.socket = Some(socket);
                self.io_error = None;
                Ok(())
            }
            Err(error) => {
                self.lost_at = Some(Instant::now());
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
                        if !spoken(&header)
                            || header.payload_type == PayloadType::RoutingActivationResponse
                        {
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

impl<'s, C: TcpConnect, const N: usize> Tester<'s, C, N> {
    /// Gives up the connection, aborting it where the tester is the one ending it, for no
    /// longer than `until`.
    async fn lose_connection(&mut self, abort: bool, until: Option<Instant>) {
        if let Some(mut socket) = self.give_up()
            && abort
        {
            match until {
                Some(until) => with_deadline(until, socket.abort()).await.ok().map(drop),
                None => socket.abort().await.ok(),
            };
        }
    }

    /// Gives up the connection, returning its socket if there was one.
    ///
    /// An outstanding request is owed its confirm: `DoIP_NO_SOCKET` if its last byte
    /// never left, `DoIP_ERROR` if it left and was never acknowledged.
    fn give_up(&mut self) -> Option<C::Socket<'s>> {
        let socket = self.socket.take();
        if socket.is_some() {
            self.lost_at = Some(Instant::now());
        }
        if let Some(outstanding) = self.exchange.outstanding.take() {
            let result = if outstanding.sent {
                DoIpResult::Error
            } else {
                DoIpResult::NoSocket
            };
            self.owed = Some(outstanding.confirm(self.sa.address(), result));
        }
        self.rx.clear();
        self.control.clear();
        self.outgoing.clear();
        socket
    }

    /// Owes the outstanding request `DoIP_TIMEOUT_A` once its time is up, and whether it
    /// did.
    ///
    /// A request none of which was written is withdrawn; otherwise the connection is
    /// given up, so that neither a late acknowledgement nor a late response can be taken
    /// for a later request's.
    async fn time_out(&mut self, until: Option<Instant>) -> bool {
        let Some(outstanding) = self.exchange.outstanding else {
            return false;
        };
        if outstanding
            .deadline
            .is_none_or(|deadline| deadline > Instant::now())
        {
            return false;
        }
        self.exchange.outstanding = None;
        self.owed = Some(outstanding.confirm(self.sa.address(), DoIpResult::TimeoutA));
        if self.outgoing.untouched() {
            self.outgoing.clear();
        } else {
            self.lose_connection(true, until).await;
        }
        true
    }
}

/// `DoIP_Data` over the tester's connection.
///
/// A connection ending is an event, never an `Err`: the confirm a request awaiting one is
/// owed, then [`ConnectionEvent::Closed`], whether the entity closed the connection, the
/// tester gave it up, or the socket failed, whose error [`Tester::io_error`] keeps. Every
/// [`next_event`](DiagnosticConnection::next_event) after that reports `Closed` again,
/// and [`request`](DiagnosticConnection::request) is [`Error::NotConnected`], until a
/// reconnect succeeds. `next_event` never returns `Err`.
impl<C: TcpConnect, const N: usize> DiagnosticConnection for Tester<'_, C, N> {
    type Error = Error;

    /// The longest PDU a request may carry, and the longest an indication delivers
    /// whole: `N` less [`DIAGNOSTIC_MESSAGE_OVERHEAD`]. Naming it for an `N` below
    /// [`MIN_N`] fails to build.
    const MAX_PDU: usize = {
        assert!(N >= MIN_N, "a Tester's N must be at least MIN_N");
        N.saturating_sub(DIAGNOSTIC_MESSAGE_OVERHEAD)
    };

    /// Queues `pdu` to `ta` as one diagnostic message, from the tester's source address,
    /// for [`DiagnosticConnection::next_event`] to write.
    ///
    /// Its [`ConnectionEvent::Confirm`] comes from the entity's acknowledgement
    /// (ISO 13400-2:2019 9.5): [`DoIpResult::Ok`] for a positive one,
    /// [`DoIpResult::Error`] for a positive one with a reserved code, and the result
    /// naming a negative one's code. A physical request's acknowledgement must come from
    /// `ta`, and nothing the tester had read before it started writing the request is
    /// its acknowledgement. A generic header negative acknowledgement confirms the
    /// request only if no alive check response was written within
    /// `A_DoIP_Diagnostic_Message` before the request or since, as it may otherwise be
    /// about that response.
    ///
    /// The confirm is [`DoIpResult::TimeoutA`] where no acknowledgement arrives within
    /// `A_DoIP_Diagnostic_Message` (Table 12) of the request's last byte, or where its
    /// bytes cannot all be written within that time of the tester starting to write
    /// them. The request is then lost: one none of which was written is withdrawn, and
    /// otherwise the tester gives the connection up, so that neither a late
    /// acknowledgement nor a late response can be taken for a later request's.
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
    /// - [`Error::MessageTooLarge`] for a `pdu` longer than
    ///   [`MAX_PDU`](DiagnosticConnection::MAX_PDU).
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
            started: None,
            deadline: None,
            sent: false,
            stale: 0,
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
    /// reserves, or [`ConnectionEvent::UnmodelledTruncated`] where it does not fit `buf`
    /// or `N` less the generic header. Ignores every other message a tester is not sent,
    /// every message in error (8.3.3), and a diagnostic message addressed to another
    /// tester. Gives up the connection on a header it cannot delimit or a protocol
    /// version Table 16 does not define for `DoIP` messages, and after a negative
    /// acknowledgement on which the entity closes its socket: diagnostic `0x02`
    /// (REQ 7.DoIP-070), and generic header `0x00` and `0x04` (Table 19).
    ///
    /// Once `deadline_ms` has passed, it still delivers what has arrived, reading the
    /// socket at most once more, and writes an alive check response it owes if the
    /// socket takes it at once, before reporting [`ConnectionEvent::Deadline`].
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
        let mut read_since_passed = false;
        loop {
            if let Some(owed) = self.owed.take() {
                return Ok(owed);
            }
            if self.socket.is_none() {
                return Ok(ConnectionEvent::Closed);
            }
            if self.time_out(until).await {
                continue;
            }
            self.exchange.start_writing(self.rx.buffered());
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
                    self.lose_connection(false, None).await;
                    continue;
                }
                Err(error) => {
                    self.io_error = Some(error);
                    self.lose_connection(false, None).await;
                    continue;
                }
            }
            let sa = self.sa.address();
            let reaction = match self.rx.next() {
                Err(_) => Reaction::Close { confirm: None },
                Ok(Next::NeedMore) => {
                    if read_since_passed {
                        return Ok(ConnectionEvent::Deadline);
                    }
                    match fill(socket, &mut self.rx, wake).await {
                        Ok(Fill::TimedOut) if passed() => {
                            return Ok(ConnectionEvent::Deadline);
                        }
                        Ok(Fill::TimedOut) => {}
                        Ok(Fill::Data) => read_since_passed = passed(),
                        Ok(Fill::Eof) => self.lose_connection(false, None).await,
                        Err(error) => {
                            self.io_error = Some(error);
                            self.lose_connection(false, None).await;
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
                    self.exchange.consumed(usize::MAX);
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
                    self.exchange.consumed(consumed);
                    reaction
                }
            };
            match reaction {
                Reaction::Ignore => {}
                Reaction::Deliver(delivered) => return Ok(delivered.into_event(buf)),
                Reaction::Close { confirm } => {
                    self.owed = confirm;
                    self.lose_connection(true, until).await;
                }
            }
        }
    }
}

impl<C: TcpConnect, const N: usize> TesterConnection for Tester<'_, C, N> {
    type ReconnectError = ConnectError<C::Error>;
    type CloseError = C::Error;

    /// Gives up the connection, if there is one, waits until [`RECONNECT_BACKOFF`] has
    /// passed since a connection was last lost, then connects and activates routing
    /// again, as [`Tester::connect`] did.
    ///
    /// The order is the point: an entity may hold this tester's address for a while
    /// after the socket closes, and refuse its activation meanwhile, so a reconnect
    /// that opened the new connection before closing the old, or straight after, would
    /// be refused. A reconnect made long after the connection was lost does not wait.
    /// A request still awaiting its confirm is confirmed with [`DoIpResult::Error`] or
    /// [`DoIpResult::NoSocket`] by the next
    /// [`next_event`](DiagnosticConnection::next_event), before anything from the new
    /// connection.
    ///
    /// # Cancel safety
    ///
    /// As for [`Tester::connect`]: no timer bounds the attempt but the back-off, so
    /// bound the whole of it by dropping the future, for example with
    /// [`embassy_time::with_timeout`]. Dropped, or failed, it leaves the tester with no
    /// connection.
    ///
    /// # Errors
    ///
    /// As for [`Tester::connect`]. After an error,
    /// [`next_event`](DiagnosticConnection::next_event) reports
    /// [`ConnectionEvent::Closed`] and [`request`](DiagnosticConnection::request) is
    /// [`Error::NotConnected`] until a reconnect succeeds.
    async fn reconnect(&mut self) -> Result<(), Self::ReconnectError> {
        self.lose_connection(true, None).await;
        if let Some(lost_at) = self.lost_at {
            Timer::at(confirm::after(lost_at, RECONNECT_BACKOFF)).await;
        }
        self.establish().await
    }

    /// Closes the connection gracefully, if there is one, through the socket's
    /// `edge_nal::TcpShutdown::close`, leaving the tester closed until a reconnect
    /// succeeds.
    ///
    /// Dropping a tester instead drops its socket, and what that does is the backend's:
    /// `edge-nal-std` closes the connection, but `edge-nal-embassy` 0.9 frees the socket
    /// before its close is sent, so the entity holds its socket for this tester until its
    /// own timers give it up.
    ///
    /// A request awaiting its confirm is confirmed with [`DoIpResult::NoSocket`] or
    /// [`DoIpResult::Error`] by the next
    /// [`next_event`](DiagnosticConnection::next_event), as when the connection is lost
    /// otherwise, which then reports [`ConnectionEvent::Closed`]. A close counts as a
    /// loss for [`RECONNECT_BACKOFF`].
    ///
    /// # Cancel safety
    ///
    /// Waits as long as the backend's close does, which may be until the entity has
    /// closed its end too. Bound it by dropping the future: dropped, it drops the socket,
    /// and the tester is closed either way.
    ///
    /// # Errors
    ///
    /// The socket's error where closing fails; the tester is closed all the same.
    async fn close(&mut self) -> Result<(), Self::CloseError> {
        match self.give_up() {
            Some(mut socket) => socket.close(Close::Both).await,
            None => Ok(()),
        }
    }
}

/// The request awaiting its acknowledgement.
#[derive(Debug, Default)]
struct Exchange {
    outstanding: Option<Outstanding>,
}

impl Exchange {
    fn deadline(&self) -> Option<Instant> {
        self.outstanding
            .and_then(|outstanding| outstanding.deadline)
    }

    /// Starts the outstanding request's timer as the tester starts writing it, with
    /// `buffered` bytes read and not yet consumed.
    fn start_writing(&mut self, buffered: usize) {
        if let Some(outstanding) = &mut self.outstanding
            && outstanding.started.is_none()
        {
            let now = Instant::now();
            outstanding.started = Some(now);
            outstanding.deadline = Some(confirm::after(now, ACK_TIMEOUT));
            outstanding.stale = buffered;
        }
    }

    /// Restarts the outstanding request's timer from its last byte, now written.
    fn mark_sent(&mut self) {
        if let Some(outstanding) = &mut self.outstanding
            && !outstanding.sent
        {
            outstanding.sent = true;
            outstanding.deadline = Some(confirm::after(Instant::now(), ACK_TIMEOUT));
        }
    }

    /// Records that `consumed` bytes of what was buffered have been taken.
    fn consumed(&mut self, consumed: usize) {
        if let Some(outstanding) = &mut self.outstanding {
            outstanding.stale = outstanding.stale.saturating_sub(consumed);
        }
    }

    /// Takes an acknowledgement from `source`, or a generic header NACK where `source`
    /// is `None`, as the outstanding request's, if it can be. `answered_at` is when the
    /// last alive check response was written.
    fn acknowledge(
        &mut self,
        sa: LogicalAddress,
        source: Option<LogicalAddress>,
        answered_at: Option<Instant>,
        result: DoIpResult,
    ) -> Option<ConnectionEvent<'static>> {
        let outstanding = self
            .outstanding
            .take_if(|outstanding| outstanding.acknowledged_by(source, answered_at))?;
        Some(outstanding.confirm(sa, result))
    }
}

#[derive(Debug, Clone, Copy)]
struct Outstanding {
    ta: LogicalAddress,
    ta_type: TaType,
    /// When the tester started writing the request; unset until then.
    started: Option<Instant>,
    deadline: Option<Instant>,
    sent: bool,
    /// Bytes read before the request was started and not yet consumed: nothing in them
    /// can be about it.
    stale: usize,
}

impl Outstanding {
    /// Whether an acknowledgement from `source`, or a generic header NACK where `source`
    /// is `None`, is this request's. A header NACK is not where an alive check response
    /// was written within `A_DoIP_Diagnostic_Message` before the request or since, as
    /// it may be about that response.
    fn acknowledged_by(
        &self,
        source: Option<LogicalAddress>,
        answered_at: Option<Instant>,
    ) -> bool {
        if self.stale > 0 {
            return false;
        }
        match source {
            Some(source) => self.ta_type != TaType::Physical || source == self.ta,
            None => self.started.is_some_and(|started| {
                answered_at
                    .is_none_or(|answered| confirm::after(answered, ACK_TIMEOUT) <= started)
            }),
        }
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
        length: usize,
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
                length,
            } => {
                let data = &buf[..copied];
                if copied == length {
                    ConnectionEvent::Unmodelled { payload_type, data }
                } else {
                    ConnectionEvent::UnmodelledTruncated {
                        payload_type,
                        data,
                        length,
                    }
                }
            }
        }
    }
}

/// Whether `header`'s protocol version is one ISO 13400-2:2019 Table 16 defines for
/// `DoIP` messages: ISO/DIS 13400-2:2010's, ISO 13400-2:2012's or its own.
fn spoken(header: &Header) -> bool {
    matches!(
        header.protocol_version,
        ProtocolVersion::V2010 | ProtocolVersion::V2012 | ProtocolVersion::V2019
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
            if ta != sa && TesterAddress::new(ta).is_ok() {
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
        (_, Ok(Payload::DiagnosticMessageAck(ack))) if ack.target_address == sa => {
            let result = if ack.ack_code == DiagnosticAckCode::RoutingConfirmationAck {
                DoIpResult::Ok
            } else {
                DoIpResult::Error
            };
            deliver_confirm(exchange.acknowledge(
                sa,
                Some(ack.source_address),
                control.answered_at(),
                result,
            ))
        }
        (_, Ok(Payload::DiagnosticMessageNack(nack))) if nack.target_address == sa => {
            let result = confirm::from_diagnostic_nack(nack.nack_code);
            let confirm = exchange.acknowledge(
                sa,
                Some(nack.source_address),
                control.answered_at(),
                result,
            );
            if result == DoIpResult::InvalidSa {
                Reaction::Close { confirm }
            } else {
                deliver_confirm(confirm)
            }
        }
        (_, Ok(Payload::DoIPNack(code))) => {
            let confirm = exchange.acknowledge(
                sa,
                None,
                control.answered_at(),
                confirm::from_header_nack(code),
            );
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
                length: header.payload_length as usize,
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
