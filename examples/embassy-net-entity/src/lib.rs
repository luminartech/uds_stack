//! An [`Entity`] on embassy-net.
//!
//! `edge-nal-embassy` 0.9's acceptor cannot be used here: its `accept` allocates a socket
//! inside the future, so dropping the future, which [`Entity`] does routinely, loses a
//! connection already made. [`Acceptor`] is the adapter instead: it owns `MCTS + 1` of
//! embassy-net's own `TcpSocket`s, keeps each listening across dropped `accept`s, and
//! hands out whichever has a connection. A socket the entity is done with returns to the
//! pool: aborted if its connection was still open, left to finish a close the entity
//! started, then listening again.
//!
//! While in the pool a socket has a timeout of [`POOL_TIMEOUT`], so a handshake whose
//! peer vanished, or a close whose peer never closes its side, gives the socket back. A
//! socket the entity holds has none: the entity keeps `DoIP`'s own timers.
//!
//! [`serve`] is what a firmware's task calls once its embassy-net `Stack` is up.

#![no_std]

use core::cell::{RefCell, RefMut};
use core::fmt;
use core::future::{Future, poll_fn};
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::pin::pin;
use core::task::Poll;

use edge_nal::{Close, Readable, TcpAccept, TcpShutdown, TcpSplit};
use embassy_net::Stack;
use embassy_net::tcp::{Error, State, TcpReader, TcpSocket, TcpWriter};
use embassy_time::Duration;
use embedded_io_async::{ErrorType, Read, Write};
use simple_doip::TCP_PORT;
use simple_doip::entity::{Entity, EntityAddress};
use simple_doip::service::{DiagnosticEntity, EntityConfig, EntityEvent};

/// How long a socket in the pool waits on a silent peer before it is aborted.
pub const POOL_TIMEOUT: Duration = Duration::from_secs(10);

/// `N` embassy-net sockets listening on one port.
pub struct Acceptor<'d, const N: usize> {
    port: u16,
    sockets: [RefCell<TcpSocket<'d>>; N],
}

impl<const N: usize> fmt::Debug for Acceptor<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Acceptor")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl<'d, const N: usize> Acceptor<'d, N> {
    /// An acceptor listening on `port` with `sockets`; an entity needs `MCTS + 1`.
    #[must_use]
    pub fn new(port: u16, sockets: [TcpSocket<'d>; N]) -> Self {
        Self {
            port,
            sockets: sockets.map(RefCell::new),
        }
    }
}

/// A connection the [`Acceptor`] handed out. Dropping it returns the socket to the pool,
/// aborting the connection unless the entity has closed it.
pub struct Accepted<'a, 'd> {
    socket: RefMut<'a, TcpSocket<'d>>,
}

impl fmt::Debug for Accepted<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accepted")
            .field("state", &self.socket.state())
            .finish()
    }
}

impl Drop for Accepted<'_, '_> {
    fn drop(&mut self) {
        if matches!(self.socket.state(), State::Established | State::CloseWait) {
            self.socket.abort();
        }
        self.socket.set_timeout(Some(POOL_TIMEOUT));
    }
}

impl<'d, const N: usize> TcpAccept for Acceptor<'d, N> {
    type Error = Error;
    type Socket<'a>
        = Accepted<'a, 'd>
    where
        Self: 'a;

    /// Cancel-safe: the sockets listen whether or not this future is polled, and one is
    /// taken from the pool only by the poll that returns it.
    async fn accept(&self) -> Result<(SocketAddr, Accepted<'_, 'd>), Error> {
        poll_fn(|cx| {
            for home in &self.sockets {
                let Ok(mut socket) = home.try_borrow_mut() else {
                    continue;
                };
                match socket.state() {
                    State::Established | State::CloseWait => {
                        socket.set_timeout(None);
                        let remote = socket.remote_endpoint().map_or(
                            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                            |endpoint| SocketAddr::new(endpoint.addr.into(), endpoint.port),
                        );
                        return Poll::Ready(Ok((remote, Accepted { socket })));
                    }
                    State::Closed | State::Listen | State::TimeWait => {
                        // Listening again on the same endpoint changes nothing; polling
                        // the accept registers this task to be woken by a connection.
                        socket.set_timeout(Some(POOL_TIMEOUT));
                        let refused = matches!(
                            pin!(socket.accept(self.port)).poll(cx),
                            Poll::Ready(Err(_))
                        );
                        if refused {
                            socket.abort();
                        }
                    }
                    _ => {
                        // Never ready in these states: the poll only registers this task
                        // to be woken when the handshake or the close moves on.
                        let _ = pin!(socket.wait_write_ready()).poll(cx);
                    }
                }
            }
            Poll::Pending
        })
        .await
    }
}

impl ErrorType for Accepted<'_, '_> {
    type Error = Error;
}

impl Read for Accepted<'_, '_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.socket.read(buf).await
    }
}

impl Write for Accepted<'_, '_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.socket.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Error> {
        self.socket.flush().await
    }
}

impl Readable for Accepted<'_, '_> {
    /// Completes on data, and also on the peer's FIN or RST, which embassy-net's own
    /// `wait_read_ready` does not.
    async fn readable(&mut self) -> Result<(), Error> {
        self.socket.read_with(|_| (0, ())).await.ok();
        Ok(())
    }
}

impl TcpShutdown for Accepted<'_, '_> {
    async fn close(&mut self, _what: Close) -> Result<(), Error> {
        let socket = &mut *self.socket;
        socket.close();
        socket.flush().await
    }

    async fn abort(&mut self) -> Result<(), Error> {
        let socket = &mut *self.socket;
        socket.abort();
        socket.flush().await
    }
}

/// The read half of an [`Accepted`] connection.
pub struct ReadHalf<'a>(TcpReader<'a>);

impl fmt::Debug for ReadHalf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadHalf").finish_non_exhaustive()
    }
}

/// The write half of an [`Accepted`] connection.
pub struct WriteHalf<'a>(TcpWriter<'a>);

impl fmt::Debug for WriteHalf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteHalf").finish_non_exhaustive()
    }
}

impl ErrorType for ReadHalf<'_> {
    type Error = Error;
}

impl Read for ReadHalf<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.0.read(buf).await
    }
}

impl Readable for ReadHalf<'_> {
    async fn readable(&mut self) -> Result<(), Error> {
        self.0.read_with(|_| (0, ())).await.ok();
        Ok(())
    }
}

impl ErrorType for WriteHalf<'_> {
    type Error = Error;
}

impl Write for WriteHalf<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Error> {
        self.0.flush().await
    }
}

impl TcpSplit for Accepted<'_, '_> {
    type Read<'a>
        = ReadHalf<'a>
    where
        Self: 'a;
    type Write<'a>
        = WriteHalf<'a>
    where
        Self: 'a;

    fn split(&mut self) -> (ReadHalf<'_>, WriteHalf<'_>) {
        let (read, write) = self.socket.split();
        (ReadHalf(read), WriteHalf(write))
    }
}

/// The entity's one connection.
pub const MCTS: usize = 1;
/// The largest message the connection carries.
pub const MAX_MESSAGE: usize = 4096;

/// Serves `DoIP` as `address` on `stack` with `MCTS + 1` sockets over the buffers given,
/// echoing every diagnostic message.
///
/// Returns only when [`DiagnosticEntity::next_event`] fails, with its error.
pub async fn serve(
    stack: Stack<'_>,
    address: EntityAddress,
    rx: &mut [[u8; MAX_MESSAGE]; MCTS + 1],
    tx: &mut [[u8; MAX_MESSAGE]; MCTS + 1],
) -> simple_doip::entity::Error<Error> {
    let [rx0, rx1] = rx;
    let [tx0, tx1] = tx;
    let acceptor = Acceptor::new(
        TCP_PORT,
        [
            TcpSocket::new(stack, rx0, tx0),
            TcpSocket::new(stack, rx1, tx1),
        ],
    );
    let mut entity =
        Entity::<_, MCTS, MAX_MESSAGE>::new(&acceptor, address, EntityConfig::default());
    let mut buf = [0u8; MAX_MESSAGE];
    let mut answer = [0u8; MAX_MESSAGE];
    loop {
        let length = match entity.next_event(&mut buf, None).await {
            Ok(EntityEvent::Indication { sa, pdu, .. }) => answer
                .get_mut(..pdu.len())
                .map(|to| to.copy_from_slice(pdu))
                .map(|()| (sa, pdu.len())),
            Ok(_) => None,
            Err(error) => return error,
        };
        if let Some((sa, length)) = length {
            entity
                .request(
                    address.physical(),
                    sa,
                    simple_doip::TaType::Physical,
                    answer.get(..length).unwrap_or_default(),
                )
                .await
                .ok();
        }
    }
}
