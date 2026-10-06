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

use crate::LogicalAddress;
use crate::RawFrame;
use crate::messages::{
    ActivationTypeCode, Header, Message, NackCode, Payload, ProtocolVersion,
    RoutingActivationResponseCode,
};
use crate::service::NotATesterAddress;

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the tester in the next commit")
)]
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
