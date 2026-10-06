//! A scripted `edge_nal::TcpConnect` backend whose sockets move a configurable number of
//! bytes per read or write and yield once before every one, so each byte can be an await
//! point; and a single-threaded executor to drive futures against it poll by poll.

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
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use edge_nal::{Close, Readable, TcpConnect, TcpShutdown, TcpSplit};
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
    outbound: Vec<u8>,
    aborted: bool,
    closed: bool,
    dropped: bool,
    reader: Option<Waker>,
}

#[derive(Debug)]
struct Shared {
    /// What each coming `connect` gets: bytes already sent, or `None` for a refusal.
    scripts: VecDeque<Option<Vec<u8>>>,
    connections: Vec<Connection>,
    piece: usize,
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
        })))
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

    /// Everything the tester has written, and forget it.
    pub fn take_written(&self) -> Vec<u8> {
        self.with(|c| std::mem::take(&mut c.outbound))
    }

    /// Whether the tester aborted, closed or dropped this connection.
    pub fn is_shut(&self) -> bool {
        self.with(|c| c.aborted || c.closed || c.dropped)
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

impl Read for MockSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, MockError> {
        yield_once().await;
        let piece = self.peer.shared.borrow().piece;
        poll_fn(|cx| {
            self.peer.with(|c| {
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
}

impl Write for MockSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, MockError> {
        yield_once().await;
        let piece = self.peer.shared.borrow().piece;
        let n = buf.len().min(piece);
        self.peer.with(|c| c.outbound.extend(&buf[..n]));
        Ok(n)
    }

    async fn flush(&mut self) -> Result<(), MockError> {
        Ok(())
    }
}

impl Readable for MockSocket {
    async fn readable(&mut self) -> Result<(), MockError> {
        poll_fn(|cx| {
            self.peer.with(|c| {
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

impl TcpShutdown for MockSocket {
    async fn close(&mut self, _what: Close) -> Result<(), MockError> {
        self.peer.with(|c| c.closed = true);
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), MockError> {
        self.peer.with(|c| c.aborted = true);
        Ok(())
    }
}

/// Unused by the tester, but required of every `edge-nal` socket.
#[derive(Debug)]
pub struct MockHalf;

impl ErrorType for MockHalf {
    type Error = MockError;
}

impl Read for MockHalf {
    async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, MockError> {
        Err(MockError)
    }
}

impl Write for MockHalf {
    async fn write(&mut self, _buf: &[u8]) -> Result<usize, MockError> {
        Err(MockError)
    }

    async fn flush(&mut self) -> Result<(), MockError> {
        Err(MockError)
    }
}

impl Readable for MockHalf {
    async fn readable(&mut self) -> Result<(), MockError> {
        Err(MockError)
    }
}

impl TcpSplit for MockSocket {
    type Read<'a> = MockHalf;
    type Write<'a> = MockHalf;

    fn split(&mut self) -> (MockHalf, MockHalf) {
        panic!("the tester must not split its socket")
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

// --- driving futures ----------------------------------------------------------------

/// Polls `future` until it completes, or panics after `limit` polls.
pub fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..100_000 {
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

/// Polls `future` until it is waiting on something only the test can give, returning
/// its output if it completed instead.
pub fn until_stalled<F: Future + ?Sized>(future: Pin<&mut F>) -> Option<F::Output> {
    poll_times(future, 10_000)
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
