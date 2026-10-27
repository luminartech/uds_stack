//! `edge-nal` sockets over the C socket calls (`uds_tcp_*`, `uds_udp_*`).
//!
//! The firmware owns every socket and queues what its stack delivers; each call here
//! either completes or reports would-block having moved nothing, which is the cancel
//! safety the entity requires. Would-block maps to `Pending`, and nothing wakes the
//! server: it is polled again on the next pass of the firmware's loop.

#![allow(unsafe_code, reason = "calls the C socket functions with Rust buffers")]

use core::future::{Future, poll_fn, ready};
use core::marker::PhantomData;
use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use core::task::Poll;

use edge_nal::{
    Close, Readable, TcpAccept, TcpShutdown, TcpSplit, UdpReceive, UdpSend, UdpSplit,
};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

use crate::sys;

/// A socket call the firmware refused: `UDS_SOCK_ERR`, the socket gone or its stack
/// failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the firmware's socket call failed")]
pub struct Failed;

impl embedded_io_async::Error for Failed {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// A socket call's byte count or status as a poll: would-block is `Pending`, another
/// negative value or a count past `cap`, the most the call could move, an error.
fn completed(rc: i32, cap: usize) -> Poll<Result<usize, Failed>> {
    match rc {
        sys::SOCK_WOULD_BLOCK => Poll::Pending,
        n => Poll::Ready(usize::try_from(n).ok().filter(|&n| n <= cap).ok_or(Failed)),
    }
}

fn v4(addr: u32, port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from_bits(addr), port))
}

/// The TCP listener the firmware accepts `DoIP` connections on.
#[derive(Debug)]
pub struct Acceptor;

impl TcpAccept for Acceptor {
    type Error = Failed;
    type Socket<'a> = Socket;

    async fn accept(&self) -> Result<(SocketAddr, Socket), Failed> {
        poll_fn(|_| {
            let (mut addr, mut port) = (0, 0);
            // SAFETY: both out-pointers are valid locals for the call.
            let rc = unsafe { sys::uds_tcp_accept(&raw mut addr, &raw mut port) };
            completed(rc, usize::MAX).map_ok(|_| (v4(addr, port), Socket(rc)))
        })
        .await
    }
}

/// An accepted connection: the socket index the firmware handed out, given back by
/// `uds_tcp_release` when it is dropped.
#[derive(Debug)]
pub struct Socket(i32);

impl Drop for Socket {
    fn drop(&mut self) {
        sys::uds_tcp_release(self.0);
    }
}

/// Either direction of a connection, borrowed from its [`Socket`] so it cannot outlive
/// the index. The halves share the index; neither waits on a wakeup, so they never
/// displace each other.
#[derive(Debug)]
pub struct Half<'a>(i32, PhantomData<&'a mut Socket>);

async fn read(sock: i32, buf: &mut [u8]) -> Result<usize, Failed> {
    poll_fn(|_| {
        // SAFETY: `buf` is valid for `buf.len()` bytes for the call.
        let rc = unsafe { sys::uds_tcp_read(sock, buf.as_mut_ptr(), buf.len()) };
        completed(rc, buf.len())
    })
    .await
}

async fn readable(sock: i32) -> Result<(), Failed> {
    poll_fn(|_| {
        if sys::uds_tcp_readable(sock) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await
}

async fn write(sock: i32, buf: &[u8]) -> Result<usize, Failed> {
    poll_fn(|_| {
        // SAFETY: `buf` is valid for `buf.len()` bytes for the call, which copies it.
        let rc = unsafe { sys::uds_tcp_write(sock, buf.as_ptr(), buf.len()) };
        completed(rc, buf.len())
    })
    .await
}

impl ErrorType for Half<'_> {
    type Error = Failed;
}

impl Read for Half<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Failed> {
        read(self.0, buf).await
    }
}

impl Readable for Half<'_> {
    async fn readable(&mut self) -> Result<(), Failed> {
        readable(self.0).await
    }
}

impl Write for Half<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Failed> {
        write(self.0, buf).await
    }

    /// Every write is already handed to the firmware's stack, which sends it.
    fn flush(&mut self) -> impl Future<Output = Result<(), Failed>> {
        ready(Ok(()))
    }
}

impl ErrorType for Socket {
    type Error = Failed;
}

impl Read for Socket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Failed> {
        read(self.0, buf).await
    }
}

impl Readable for Socket {
    async fn readable(&mut self) -> Result<(), Failed> {
        readable(self.0).await
    }
}

impl Write for Socket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Failed> {
        write(self.0, buf).await
    }

    /// Every write is already handed to the firmware's stack, which sends it.
    fn flush(&mut self) -> impl Future<Output = Result<(), Failed>> {
        ready(Ok(()))
    }
}

impl TcpSplit for Socket {
    type Read<'a> = Half<'a>;
    type Write<'a> = Half<'a>;

    fn split(&mut self) -> (Half<'_>, Half<'_>) {
        (Half(self.0, PhantomData), Half(self.0, PhantomData))
    }
}

impl TcpShutdown for Socket {
    fn close(&mut self, _what: Close) -> impl Future<Output = Result<(), Failed>> {
        sys::uds_tcp_close(self.0, false);
        ready(Ok(()))
    }

    fn abort(&mut self) -> impl Future<Output = Result<(), Failed>> {
        sys::uds_tcp_close(self.0, true);
        ready(Ok(()))
    }
}

/// The UDP socket `DoIP` discovery runs on: requests in, answers and announcements out.
#[derive(Debug)]
pub struct Datagrams;

impl ErrorType for Datagrams {
    type Error = Failed;
}

impl UdpReceive for Datagrams {
    async fn receive(&mut self, buf: &mut [u8]) -> Result<(usize, SocketAddr), Failed> {
        poll_fn(|_| {
            let (mut addr, mut port) = (0, 0);
            // SAFETY: `buf` is valid for `buf.len()` bytes and both
            // out-pointers are valid locals for the call.
            let rc = unsafe {
                sys::uds_udp_recv(buf.as_mut_ptr(), buf.len(), &raw mut addr, &raw mut port)
            };
            completed(rc, buf.len()).map_ok(|len| (len, v4(addr, port)))
        })
        .await
    }
}

impl Readable for Datagrams {
    async fn readable(&mut self) -> Result<(), Failed> {
        poll_fn(|_| {
            if sys::uds_udp_readable() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

impl UdpSend for Datagrams {
    /// Sent when called: the firmware's call completes or fails at once.
    fn send(
        &mut self,
        remote: SocketAddr,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Failed>> {
        let SocketAddr::V4(remote) = remote else {
            return ready(Err(Failed));
        };
        // SAFETY: `data` is valid for `data.len()` bytes for the call.
        let rc = unsafe {
            sys::uds_udp_send(
                remote.ip().to_bits(),
                remote.port(),
                data.as_ptr(),
                data.len(),
            )
        };
        ready(if rc < 0 { Err(Failed) } else { Ok(()) })
    }
}

impl UdpSplit for Datagrams {
    type Receive<'a> = Datagrams;
    type Send<'a> = Datagrams;

    fn split(&mut self) -> (Datagrams, Datagrams) {
        (Datagrams, Datagrams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_within_the_buffer_completes() {
        assert_eq!(completed(0, 4), Poll::Ready(Ok(0)));
        assert_eq!(completed(4, 4), Poll::Ready(Ok(4)));
    }

    #[test]
    fn would_block_is_pending() {
        assert_eq!(completed(sys::SOCK_WOULD_BLOCK, 4), Poll::Pending);
    }

    #[test]
    fn an_error_or_a_count_past_the_buffer_fails() {
        assert_eq!(completed(-2, 4), Poll::Ready(Err(Failed)));
        assert_eq!(completed(5, 4), Poll::Ready(Err(Failed)));
    }
}
