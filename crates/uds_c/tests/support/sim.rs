//! The socket and clock calls of `uds_c.h`, over in-memory buffers.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::vec::Vec;

/// `uds_now_ms`: what the test sets.
pub static NOW_MS: AtomicU32 = AtomicU32::new(0);

/// The most bytes a write takes, after first answering would-block; 0 for no limit.
/// Simulates the firmware's send buffer filling.
pub static WRITE_CAP: AtomicUsize = AtomicUsize::new(0);

/// Whether the stack holds what `uds_tcp_write` takes from now on, transmitting none of
/// it, so `uds_tcp_transmitted` lags the bytes written.
static HOLD: AtomicBool = AtomicBool::new(false);

/// The stack holds what it takes from now on, or transmits everything taken.
pub fn hold_transmission(hold: bool) {
    HOLD.store(hold, Ordering::Relaxed);
}

/// The socket call that answers `UDS_SOCK_ERR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// None: every call behaves.
    None,
    /// `uds_tcp_accept`.
    Accept,
    /// `uds_tcp_read`.
    Read,
    /// `uds_tcp_write`.
    Write,
}

static FAIL: Mutex<Fail> = Mutex::new(Fail::None);

/// Makes `call` answer `UDS_SOCK_ERR` from now on.
pub fn fail(call: Fail) {
    *FAIL.lock().unwrap_or_else(PoisonError::into_inner) = call;
}

fn failing(call: Fail) -> bool {
    *FAIL.lock().unwrap_or_else(PoisonError::into_inner) == call
}

/// How the ECU closed the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// `uds_tcp_close` without `reset`.
    Graceful,
    /// `uds_tcp_close` with `reset`, or a release of an open socket.
    Reset,
}

/// The tester's connection, from the ECU's side.
#[derive(Debug, Default)]
pub struct Connection {
    /// The socket index the server was handed; an earlier connection's index reads as
    /// closed.
    sock: i32,
    accepted: bool,
    rx: VecDeque<u8>,
    tx: Vec<u8>,
    /// How much of `tx` the stack has transmitted.
    transmitted: usize,
    closed: Option<Closed>,
    /// The last write answered would-block, under `WRITE_CAP`.
    blocked: bool,
}

static TCP: Mutex<Option<Connection>> = Mutex::new(None);

const WOULD_BLOCK: i32 = -1;
const ERR: i32 = -2;

/// Forgets the connection, the write cap, any held transmission and any failing call.
pub fn reset() {
    *TCP.lock().unwrap_or_else(PoisonError::into_inner) = None;
    WRITE_CAP.store(0, Ordering::Relaxed);
    hold_transmission(false);
    fail(Fail::None);
}

/// The tester connects, under a socket index no earlier connection had; the server
/// accepts it on its next poll.
pub fn connect() {
    static NEXT_SOCK: AtomicI32 = AtomicI32::new(0);
    *TCP.lock().unwrap() = Some(Connection {
        sock: NEXT_SOCK.fetch_add(1, Ordering::Relaxed),
        ..Connection::default()
    });
}

/// The tester sends `bytes`.
pub fn send(bytes: &[u8]) {
    TCP.lock().unwrap().as_mut().unwrap().rx.extend(bytes);
}

/// How the ECU has closed the connection, if it has.
pub fn closed() -> Option<Closed> {
    TCP.lock().unwrap().as_ref().and_then(|c| c.closed)
}

/// Every whole frame the ECU sent on the connection, as (payload type, payload).
pub fn sent() -> Vec<(u16, Vec<u8>)> {
    let tcp = TCP.lock().unwrap();
    let tx = &tcp.as_ref().unwrap().tx;
    let mut frames = Vec::new();
    let mut at = 0;
    while at + 8 <= tx.len() {
        let len =
            usize::try_from(u32::from_be_bytes(tx[at + 4..at + 8].try_into().unwrap()))
                .unwrap();
        if at + 8 + len > tx.len() {
            break;
        }
        frames.push((
            u16::from_be_bytes([tx[at + 2], tx[at + 3]]),
            tx[at + 8..at + 8 + len].to_vec(),
        ));
        at += 8 + len;
    }
    frames
}

fn with<R>(f: impl FnOnce(&mut Connection) -> R) -> Option<R> {
    TCP.lock().unwrap().as_mut().map(f)
}

/// `f` on the connection behind `sock`, or `None` for an earlier one.
fn on<R>(sock: i32, f: impl FnOnce(&mut Connection) -> R) -> Option<R> {
    TCP.lock()
        .unwrap()
        .as_mut()
        .filter(|c| c.sock == sock)
        .map(f)
}

#[unsafe(no_mangle)]
extern "C" fn uds_now_ms() -> u32 {
    NOW_MS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_accept(peer_addr: *mut u32, peer_port: *mut u16) -> i32 {
    if failing(Fail::Accept) {
        return ERR;
    }
    with(|c| {
        if c.accepted {
            return WOULD_BLOCK;
        }
        c.accepted = true;
        // SAFETY: the server passes valid out-pointers (uds_c.h).
        unsafe {
            *peer_addr = 0xC0A8_0A01;
            *peer_port = 50_000;
        }
        c.sock
    })
    .unwrap_or(WOULD_BLOCK)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_readable(sock: i32) -> bool {
    on(sock, |c| !c.rx.is_empty() || c.closed.is_some()).unwrap_or(true)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_read(sock: i32, buf: *mut u8, cap: usize) -> i32 {
    on(sock, |c| {
        if c.closed.is_some() || failing(Fail::Read) {
            return ERR;
        }
        if c.rx.is_empty() {
            return WOULD_BLOCK;
        }
        let n = cap.min(c.rx.len());
        for (i, byte) in c.rx.drain(..n).enumerate() {
            // SAFETY: the server passes `cap` writable bytes and `i < cap`.
            unsafe { *buf.add(i) = byte };
        }
        i32::try_from(n).unwrap()
    })
    .unwrap_or(ERR)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_write(sock: i32, buf: *const u8, len: usize) -> i32 {
    on(sock, |c| {
        if c.closed.is_some() || failing(Fail::Write) {
            return ERR;
        }
        let cap = WRITE_CAP.load(Ordering::Relaxed);
        let n = if cap == 0 {
            len
        } else {
            c.blocked = !c.blocked;
            if c.blocked {
                return WOULD_BLOCK;
            }
            len.min(cap)
        };
        // SAFETY: the server passes `len` readable bytes and `n <= len`.
        c.tx.extend_from_slice(unsafe { core::slice::from_raw_parts(buf, n) });
        if !HOLD.load(Ordering::Relaxed) {
            c.transmitted = c.tx.len();
        }
        i32::try_from(n).unwrap()
    })
    .unwrap_or(ERR)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_transmitted(sock: i32) -> u64 {
    on(sock, |c| {
        if !HOLD.load(Ordering::Relaxed) {
            c.transmitted = c.tx.len();
        }
        u64::try_from(c.transmitted).unwrap()
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_close(sock: i32, reset: bool) {
    on(sock, |c| {
        c.closed.get_or_insert(if reset {
            Closed::Reset
        } else {
            Closed::Graceful
        });
    });
}

#[unsafe(no_mangle)]
extern "C" fn uds_tcp_release(sock: i32) {
    on(sock, |c| {
        c.closed.get_or_insert(Closed::Reset);
    });
}

#[unsafe(no_mangle)]
extern "C" fn uds_udp_readable() -> bool {
    false
}

#[unsafe(no_mangle)]
extern "C" fn uds_udp_recv(
    _buf: *mut u8,
    _cap: usize,
    _src_addr: *mut u32,
    _src_port: *mut u16,
) -> i32 {
    WOULD_BLOCK
}

#[unsafe(no_mangle)]
extern "C" fn uds_udp_send(
    _dst_addr: u32,
    _dst_port: u16,
    _buf: *const u8,
    _len: usize,
) -> i32 {
    0
}
