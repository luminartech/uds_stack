//! The scripted socket the entity and tester tests run on, held to what they rely on:
//! bytes cross both ways, readiness follows data and end of stream, an operation dropped
//! before it completes moves nothing, and a split socket's halves wait independently.

// Test code; see `golden_vectors.rs` for why the workspace lint standard is relaxed here.
#![expect(clippy::unwrap_used)]

mod support;

use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Wake, Waker};

use edge_nal::{Readable, TcpAccept, TcpSplit};
use embedded_io_async::{Read, Write};
use support::mock_stack::{MockSocket, MockStack, poll_times, run};

fn accepted(stack: &MockStack) -> MockSocket {
    run(stack.accept()).unwrap().1
}

#[test]
fn bytes_cross_both_ways() {
    let stack = MockStack::new(4096);
    let peer = stack.dial();
    let mut socket = accepted(&stack);

    peer.send(b"abc");
    let mut buf = [0u8; 8];
    let read = run(socket.read(&mut buf)).unwrap();
    assert_eq!(buf.get(..read), Some(&b"abc"[..]));

    assert_eq!(run(socket.write(b"xyz")).unwrap(), 3);
    assert_eq!(peer.take_written(), b"xyz");
}

#[test]
fn readable_waits_for_data_or_end_of_stream() {
    let stack = MockStack::new(4096);
    let peer = stack.dial();
    let mut socket = accepted(&stack);
    assert!(poll_times(pin!(socket.readable()), 100).is_none());

    peer.send(b"a");
    assert!(poll_times(pin!(socket.readable()), 1).is_some());

    let mut buf = [0u8; 1];
    run(socket.read(&mut buf)).unwrap();
    peer.eof();
    assert!(poll_times(pin!(socket.readable()), 1).is_some());
    assert_eq!(run(socket.read(&mut buf)).unwrap(), 0);
}

#[test]
fn a_dropped_read_or_write_moves_nothing() {
    let stack = MockStack::new(4096);
    let peer = stack.dial();
    let mut socket = accepted(&stack);
    peer.send(b"ab");

    let mut buf = [0u8; 8];
    assert!(poll_times(pin!(socket.read(&mut buf)), 1).is_none());
    assert!(poll_times(pin!(socket.write(b"xyz")), 1).is_none());

    assert_eq!(peer.take_written(), b"");
    let read = run(socket.read(&mut buf)).unwrap();
    assert_eq!(buf.get(..read), Some(&b"ab"[..]));
}

#[test]
fn a_dropped_accept_loses_no_connection() {
    let stack = MockStack::new(4096);
    stack.dial();

    assert!(poll_times(pin!(stack.accept()), 1).is_none());

    assert!(run(stack.accept()).is_ok());
}

struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn flag() -> (Arc<Flag>, Waker) {
    let flag = Arc::new(Flag(AtomicBool::new(false)));
    let waker = Waker::from(flag.clone());
    (flag, waker)
}

#[test]
fn a_split_sockets_halves_are_woken_each_for_its_own_direction() {
    let stack = MockStack::new(4096);
    let peer = stack.dial();
    let mut socket = accepted(&stack);
    peer.stall_writes();
    let (mut reader, mut writer) = socket.split();
    let (read_woken, read_waker) = flag();
    let (write_woken, write_waker) = flag();
    let mut buf = [0u8; 8];
    let mut read = pin!(reader.read(&mut buf));
    let mut write = pin!(writer.write(b"xyz"));
    for _ in 0..2 {
        assert!(
            read.as_mut()
                .poll(&mut Context::from_waker(&read_waker))
                .is_pending()
        );
        assert!(
            write
                .as_mut()
                .poll(&mut Context::from_waker(&write_waker))
                .is_pending()
        );
    }
    read_woken.0.store(false, Ordering::SeqCst);
    write_woken.0.store(false, Ordering::SeqCst);

    peer.send(b"a");
    assert!(read_woken.0.load(Ordering::SeqCst));
    assert!(!write_woken.0.load(Ordering::SeqCst));

    peer.resume_writes();
    assert!(write_woken.0.load(Ordering::SeqCst));
}
