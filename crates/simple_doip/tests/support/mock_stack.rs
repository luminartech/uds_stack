//! A scripted `edge_nal` backend, `TcpConnect` for a tester and `TcpAccept` for an
//! entity, whose sockets move a configurable number of bytes per read or write and yield
//! once before every one, so each byte can be an await point; and a single-threaded
//! executor to drive futures against it poll by poll.
//!
//! [`MockPeer`] is the far end of a socket: for a tester's socket the entity, for an
//! entity's socket the tester.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
// `dead_code`: each test binary that includes this module uses a different part of it.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::struct_excessive_bools,
    clippy::unused_async_trait_impl
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Wake, Waker};

use edge_nal::{Close, Readable, TcpAccept, TcpConnect, TcpShutdown, TcpSplit};
use embassy_time::{Duration, MockDriver};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
use simple_doip::LogicalAddress;
use simple_doip::messages::{
    DiagnosticNackCode, Message, NackCode, PayloadType, ProtocolVersion,
    RoutingActivationResponseCode,
};
use simple_doip::wire::{Encode, SliceSink};

/// The mock's only error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock socket error")
    }
}

impl std::error::Error for MockError {}

impl embedded_io_async::Error for MockError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// One connection the tester made: what the peer has scripted for it and what the
/// tester did to it.
#[derive(Debug, Default)]
struct Connection {
    inbound: VecDeque<u8>,
    eof: bool,
    read_error: bool,
    write_error: bool,
    write_stall: bool,
    write_budget: Option<usize>,
    write_zero: bool,
    close_stall: bool,
    abort_stall: bool,
    outbound: Vec<u8>,
    /// How many bytes each write that wrote any carried.
    writes: Vec<usize>,
    aborted: bool,
    closed: bool,
    dropped: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
}

#[derive(Debug)]
struct Shared {
    /// What each coming `connect` gets: bytes already sent, or `None` for a refusal.
    scripts: VecDeque<Option<Vec<u8>>>,
    connections: Vec<Connection>,
    piece: usize,
    /// Whether a read yields once even when bytes are waiting.
    read_yields: bool,
    /// Connections dialled and not yet accepted.
    incoming: VecDeque<usize>,
    acceptor: Option<Waker>,
    /// Whether the next `accept` fails.
    accept_fails: bool,
}

/// The stack: every `connect` opens a new scripted [`Connection`].
#[derive(Debug, Clone)]
pub struct MockStack(Rc<RefCell<Shared>>);

impl MockStack {
    /// A stack moving at most `piece` bytes per read or write.
    pub fn new(piece: usize) -> Self {
        Self(Rc::new(RefCell::new(Shared {
            scripts: VecDeque::new(),
            connections: Vec::new(),
            piece,
            read_yields: true,
            incoming: VecDeque::new(),
            acceptor: None,
            accept_fails: false,
        })))
    }

    /// A stack like [`Self::new`] whose reads complete on their first poll when bytes are
    /// waiting, as a real stack's do.
    pub fn eager(piece: usize) -> Self {
        let stack = Self::new(piece);
        stack.0.borrow_mut().read_yields = false;
        stack
    }

    /// The next `connect` fails.
    pub fn refuse_next_connect(&self) {
        self.0.borrow_mut().scripts.push_back(None);
    }

    /// How many connections have been opened.
    pub fn connects(&self) -> usize {
        self.0.borrow().connections.len()
    }

    /// The peer end of connection `index`.
    pub fn peer(&self, index: usize) -> MockPeer {
        assert!(index < self.connects(), "no connection {index}");
        MockPeer {
            shared: self.0.clone(),
            index,
        }
    }

    /// The peer end of the latest connection.
    pub fn latest(&self) -> MockPeer {
        self.peer(self.connects() - 1)
    }

    /// A tester connects to the entity accepting on this stack; its end of the
    /// connection.
    pub fn dial(&self) -> MockPeer {
        let mut shared = self.0.borrow_mut();
        shared.connections.push(Connection::default());
        let index = shared.connections.len() - 1;
        shared.incoming.push_back(index);
        if let Some(waker) = shared.acceptor.take() {
            waker.wake();
        }
        drop(shared);
        self.peer(index)
    }

    /// The next `accept` fails.
    pub fn fail_next_accept(&self) {
        let mut shared = self.0.borrow_mut();
        shared.accept_fails = true;
        if let Some(waker) = shared.acceptor.take() {
            waker.wake();
        }
    }

    /// Bytes `connect` delivers on the next connection before the test touches it.
    pub fn script_next(&self, bytes: &[u8]) {
        self.0.borrow_mut().scripts.push_back(Some(bytes.to_vec()));
    }
}

/// The entity's end of one connection.
#[derive(Debug, Clone)]
pub struct MockPeer {
    shared: Rc<RefCell<Shared>>,
    index: usize,
}

impl MockPeer {
    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        f(&mut self.shared.borrow_mut().connections[self.index])
    }

    fn wake_reader(&self) {
        if let Some(waker) = self.with(|c| c.reader.take()) {
            waker.wake();
        }
    }

    /// The entity sends `bytes`.
    pub fn send(&self, bytes: &[u8]) {
        self.with(|c| c.inbound.extend(bytes));
        self.wake_reader();
    }

    /// The entity closes its end.
    pub fn eof(&self) {
        self.with(|c| c.eof = true);
        self.wake_reader();
    }

    /// The tester's next read fails.
    pub fn fail_reads(&self) {
        self.with(|c| c.read_error = true);
        self.wake_reader();
    }

    /// The tester's next write fails.
    pub fn fail_writes(&self) {
        self.with(|c| c.write_error = true);
    }

    /// The tester's writes pend from now on, as when the entity stops reading and its
    /// receive window fills.
    pub fn stall_writes(&self) {
        self.with(|c| c.write_stall = true);
    }

    /// The tester's writes take `bytes` more bytes, then pend from then on.
    pub fn stall_writes_after(&self, bytes: usize) {
        self.with(|c| c.write_budget = Some(bytes));
    }

    /// Ends [`Self::stall_writes`] and [`Self::stall_writes_after`], waking a write
    /// waiting on either.
    pub fn resume_writes(&self) {
        let writer = self.with(|c| {
            c.write_stall = false;
            c.write_budget = None;
            c.writer.take()
        });
        if let Some(waker) = writer {
            waker.wake();
        }
    }

    /// The tester's writes complete having written nothing, as on a closed socket.
    pub fn write_nothing(&self) {
        self.with(|c| c.write_zero = true);
    }

    /// The tester's graceful close pends from now on, as when the entity is slow to close
    /// its end.
    pub fn stall_closes(&self) {
        self.with(|c| c.close_stall = true);
    }

    /// The tester's aborts pend from now on, as when the stack cannot send the reset.
    pub fn stall_aborts(&self) {
        self.with(|c| c.abort_stall = true);
    }

    /// Whether the socket's owner dropped it.
    pub fn is_dropped(&self) -> bool {
        self.with(|c| c.dropped)
    }

    /// Everything the tester has written, and forget it.
    pub fn take_written(&self) -> Vec<u8> {
        self.with(|c| std::mem::take(&mut c.outbound))
    }

    /// How many bytes each of the tester's writes carried, and forget them.
    pub fn take_writes(&self) -> Vec<usize> {
        self.with(|c| std::mem::take(&mut c.writes))
    }

    /// Whether the tester aborted, closed or dropped this connection.
    pub fn is_shut(&self) -> bool {
        self.with(|c| c.aborted || c.closed || c.dropped)
    }

    /// Whether the tester closed this connection gracefully.
    pub fn is_closed(&self) -> bool {
        self.with(|c| c.closed)
    }

    /// Whether the tester aborted this connection, rather than only dropping it.
    pub fn is_aborted(&self) -> bool {
        self.with(|c| c.aborted)
    }

    /// Whether the tester has read everything the entity sent.
    pub fn all_read(&self) -> bool {
        self.with(|c| c.inbound.is_empty())
    }
}

/// The tester's end of one connection.
#[derive(Debug)]
pub struct MockSocket {
    peer: MockPeer,
}

impl Drop for MockSocket {
    fn drop(&mut self) {
        self.peer.with(|c| c.dropped = true);
    }
}

/// A future that is pending once, then ready: one await point.
async fn yield_once() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

impl ErrorType for MockSocket {
    type Error = MockError;
}

/// The socket operations, shared by a socket and its halves.
impl MockPeer {
    async fn read(&self, buf: &mut [u8]) -> Result<usize, MockError> {
        let (piece, read_yields) = {
            let shared = self.shared.borrow();
            (shared.piece, shared.read_yields)
        };
        if read_yields || self.with(|c| c.inbound.is_empty()) {
            yield_once().await;
        }
        poll_fn(|cx| {
            self.with(|c| {
                if c.read_error {
                    return Poll::Ready(Err(MockError));
                }
                if !c.inbound.is_empty() {
                    let n = buf.len().min(piece).min(c.inbound.len());
                    for (slot, byte) in buf.iter_mut().zip(c.inbound.drain(..n)) {
                        *slot = byte;
                    }
                    return Poll::Ready(Ok(n));
                }
                if c.eof {
                    return Poll::Ready(Ok(0));
                }
                c.reader = Some(cx.waker().clone());
                Poll::Pending
            })
        })
        .await
    }

    async fn write(&self, buf: &[u8]) -> Result<usize, MockError> {
        yield_once().await;
        let piece = self.shared.borrow().piece;
        let n = buf.len().min(piece);
        poll_fn(|cx| {
            self.with(|c| {
                if c.write_error {
                    return Poll::Ready(Err(MockError));
                }
                if c.write_stall || c.write_budget == Some(0) {
                    c.writer = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                let n = c.write_budget.map_or(n, |budget| n.min(budget));
                if let Some(budget) = &mut c.write_budget {
                    *budget -= n;
                }
                if c.write_zero {
                    return Poll::Ready(Ok(0));
                }
                c.outbound.extend(&buf[..n]);
                c.writes.push(n);
                Poll::Ready(Ok(n))
            })
        })
        .await
    }

    async fn readable(&self) -> Result<(), MockError> {
        poll_fn(|cx| {
            self.with(|c| {
                if c.read_error || c.eof || !c.inbound.is_empty() {
                    Poll::Ready(Ok(()))
                } else {
                    c.reader = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
        })
        .await
    }
}

impl Read for MockSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockError> {
        self.peer.read(buf).await
    }
}

impl Write for MockSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError> {
        self.peer.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), MockError> {
        Ok(())
    }
}

impl Readable for MockSocket {
    async fn readable(&mut self) -> Result<(), MockError> {
        self.peer.readable().await
    }
}

impl TcpShutdown for MockSocket {
    async fn close(&mut self, _what: Close) -> Result<(), MockError> {
        poll_fn(|_| {
            self.peer.with(|c| {
                if c.close_stall {
                    Poll::Pending
                } else {
                    c.closed = true;
                    Poll::Ready(Ok(()))
                }
            })
        })
        .await
    }

    async fn abort(&mut self) -> Result<(), MockError> {
        poll_fn(|_| {
            self.peer.with(|c| {
                if c.abort_stall {
                    Poll::Pending
                } else {
                    c.aborted = true;
                    Poll::Ready(Ok(()))
                }
            })
        })
        .await
    }
}

/// One half of a split [`MockSocket`]: reads and writes the same connection, each half
/// with its own waker.
#[derive(Debug)]
pub struct MockHalf {
    peer: MockPeer,
}

impl ErrorType for MockHalf {
    type Error = MockError;
}

impl Read for MockHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockError> {
        self.peer.read(buf).await
    }
}

impl Write for MockHalf {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError> {
        self.peer.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), MockError> {
        Ok(())
    }
}

impl Readable for MockHalf {
    async fn readable(&mut self) -> Result<(), MockError> {
        self.peer.readable().await
    }
}

impl TcpSplit for MockSocket {
    type Read<'a> = MockHalf;
    type Write<'a> = MockHalf;

    fn split(&mut self) -> (MockHalf, MockHalf) {
        let half = || MockHalf {
            peer: self.peer.clone(),
        };
        (half(), half())
    }
}

impl TcpConnect for MockStack {
    type Error = MockError;
    type Socket<'a> = MockSocket;

    async fn connect(&self, _remote: SocketAddr) -> Result<MockSocket, MockError> {
        yield_once().await;
        let script = self
            .0
            .borrow_mut()
            .scripts
            .pop_front()
            .unwrap_or(Some(Vec::new()));
        let Some(inbound) = script else {
            return Err(MockError);
        };
        let mut connection = Connection::default();
        connection.inbound.extend(inbound);
        self.0.borrow_mut().connections.push(connection);
        Ok(MockSocket {
            peer: self.latest(),
        })
    }
}

impl TcpAccept for MockStack {
    type Error = MockError;
    type Socket<'a> = MockSocket;

    /// Cancel-safe: a connection is taken from the queue only by the poll that returns it.
    async fn accept(&self) -> Result<(SocketAddr, MockSocket), MockError> {
        yield_once().await;
        poll_fn(|cx| {
            let mut shared = self.0.borrow_mut();
            if std::mem::take(&mut shared.accept_fails) {
                return Poll::Ready(Err(MockError));
            }
            let Some(index) = shared.incoming.pop_front() else {
                shared.acceptor = Some(cx.waker().clone());
                return Poll::Pending;
            };
            drop(shared);
            let peer = self.peer(index);
            Poll::Ready(Ok((([127, 0, 0, 1], 40_000).into(), MockSocket { peer })))
        })
        .await
    }
}

// --- driving futures ----------------------------------------------------------------

/// Polls `future` until it completes, or panics after `limit` polls.
pub fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..10_000 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
    panic!("the future did not complete: it is waiting on something the test never gives")
}

/// Polls `future` `polls` times, returning its output if it completed.
pub fn poll_times<F: Future + ?Sized>(
    mut future: Pin<&mut F>,
    polls: usize,
) -> Option<F::Output> {
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..polls {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return Some(output);
        }
    }
    None
}

struct WakeFlag(AtomicBool);

impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Under Miri, `Waker::will_wake` is false for a clone of the same waker, so
/// `embassy-time`'s timer queue evicts and wakes on every poll of a future holding a
/// timer; past this many wakes in a row, the future is taken to be waiting.
const MIRI_WAKES_BEFORE_STALLED: usize = 64;

/// Polls `future` until it returns `Pending` without waking itself, returning its output
/// if it completed instead.
///
/// # Panics
///
/// If the future keeps waking itself without completing: a busy loop.
pub fn until_stalled<F: Future + ?Sized>(mut future: Pin<&mut F>) -> Option<F::Output> {
    let flag = Arc::new(WakeFlag(AtomicBool::new(false)));
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    for polls in 1..=200_000 {
        flag.0.store(false, Ordering::SeqCst);
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return Some(output);
        }
        if !flag.0.load(Ordering::SeqCst)
            || (cfg!(miri) && polls >= MIRI_WAKES_BEFORE_STALLED)
        {
            return None;
        }
    }
    panic!("the future keeps waking itself without completing: a busy loop")
}

// --- the clock --------------------------------------------------------------------------

static CLOCK: Mutex<()> = Mutex::new(());

/// Exclusive use of the process-wide mock clock, reset to zero.
pub fn clock() -> MutexGuard<'static, ()> {
    let guard = CLOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    MockDriver::get().reset();
    guard
}

/// Moves the mock clock forward.
pub fn advance(duration: Duration) {
    MockDriver::get().advance(duration);
}

// --- frames an entity sends --------------------------------------------------------------

pub const TESTER: LogicalAddress = LogicalAddress(0x0E00);
pub const ENTITY: LogicalAddress = LogicalAddress(0x0001);

fn encode(message: &Message<'_>) -> Vec<u8> {
    let mut buf = vec![0; message.encoded_size().unwrap()];
    message.encode(&mut SliceSink::new(&mut buf)).unwrap();
    buf
}

/// A routing activation response to [`TESTER`] carrying `code`.
pub fn activation_response(code: u8) -> Vec<u8> {
    activation_response_for(TESTER, code)
}

/// A routing activation response to `tester` carrying `code`.
pub fn activation_response_for(tester: LogicalAddress, code: u8) -> Vec<u8> {
    encode(&Message::routing_activation_response(
        ProtocolVersion::V2019,
        tester,
        ENTITY,
        RoutingActivationResponseCode::from(code),
        [0; 4],
        None,
    ))
}

/// The routing activation request [`TESTER`] sends.
pub fn activation_request() -> Vec<u8> {
    vec![
        0x03, 0xFC, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]
}

/// A diagnostic message.
pub fn diagnostic(sa: LogicalAddress, ta: LogicalAddress, pdu: &[u8]) -> Vec<u8> {
    encode(&Message::diagnostic_message(
        ProtocolVersion::V2019,
        sa,
        ta,
        pdu,
    ))
}

/// A positive diagnostic message acknowledgement from `sa` to `ta`.
pub fn ack(sa: LogicalAddress, ta: LogicalAddress) -> Vec<u8> {
    encode(&Message::diagnostic_message_ack(
        ProtocolVersion::V2019,
        sa,
        ta,
        &[],
    ))
}

/// A negative diagnostic message acknowledgement from `sa` to `ta`.
pub fn nack(sa: LogicalAddress, ta: LogicalAddress, code: u8) -> Vec<u8> {
    encode(&Message::diagnostic_message_nack(
        ProtocolVersion::V2019,
        sa,
        ta,
        DiagnosticNackCode::from(code),
        &[],
    ))
}

/// An alive check request.
pub fn alive_check_request() -> Vec<u8> {
    encode(&Message::alive_check_request(ProtocolVersion::V2019))
}

/// The alive check response [`TESTER`] sends.
pub fn alive_check_response() -> Vec<u8> {
    encode(&Message::alive_check_response(
        ProtocolVersion::V2019,
        TESTER,
    ))
}

/// A generic header negative acknowledgement.
pub fn header_nack(code: u8) -> Vec<u8> {
    let mut frame = vec![0x03, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
    frame.push(u8::from(NackCode::from(code)));
    frame
}

/// A frame of any payload type.
pub fn raw(payload_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x03, 0xFC];
    frame.extend(u16::from(PayloadType::from(payload_type)).to_be_bytes());
    frame.extend(u32::try_from(payload.len()).unwrap().to_be_bytes());
    frame.extend(payload);
    frame
}

/// `frame` with its protocol version, and the inverse, set to `version`.
pub fn versioned(mut frame: Vec<u8>, version: u8) -> Vec<u8> {
    frame[0] = version;
    frame[1] = !version;
    frame
}
