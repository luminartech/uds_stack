//! A listening socket and a datagram socket whose every answer is opaque to the
//! optimizer, so that no path through the entity is compiled out.

use core::future::{Future, Ready, ready};
use core::hint::black_box;
use core::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use edge_nal::{
    Close, Readable, TcpAccept, TcpShutdown, TcpSplit, UdpReceive, UdpSend, UdpSplit,
};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};

#[derive(Debug)]
pub(super) struct Acceptor;

#[derive(Debug)]
pub(super) struct Socket;

#[derive(Debug)]
pub(super) struct Datagrams;

#[derive(Debug)]
pub(super) struct Failed;

impl core::fmt::Display for Failed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the stub socket failed")
    }
}

impl core::error::Error for Failed {}

impl embedded_io_async::Error for Failed {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

fn peer() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::from_bits(black_box(0)),
        black_box(0),
    ))
}

fn outcome<T>(value: T) -> Ready<Result<T, Failed>> {
    ready(if black_box(true) {
        Ok(value)
    } else {
        Err(Failed)
    })
}

impl TcpAccept for Acceptor {
    type Error = Failed;
    type Socket<'a> = Socket;

    fn accept(&self) -> impl Future<Output = Result<(SocketAddr, Socket), Failed>> {
        outcome((peer(), Socket))
    }
}

impl ErrorType for Socket {
    type Error = Failed;
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Failed>> {
        buf.fill(black_box(0));
        outcome(black_box(buf.len()))
    }
}

impl Readable for Socket {
    fn readable(&mut self) -> impl Future<Output = Result<(), Failed>> {
        outcome(())
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> impl Future<Output = Result<usize, Failed>> {
        outcome(black_box(buf).len())
    }

    fn flush(&mut self) -> impl Future<Output = Result<(), Failed>> {
        outcome(())
    }
}

impl TcpSplit for Socket {
    type Read<'a> = Socket;
    type Write<'a> = Socket;

    fn split(&mut self) -> (Socket, Socket) {
        (Socket, Socket)
    }
}

impl TcpShutdown for Socket {
    fn close(&mut self, _what: Close) -> impl Future<Output = Result<(), Failed>> {
        outcome(())
    }

    fn abort(&mut self) -> impl Future<Output = Result<(), Failed>> {
        outcome(())
    }
}

impl ErrorType for Datagrams {
    type Error = Failed;
}

impl UdpReceive for Datagrams {
    fn receive(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<(usize, SocketAddr), Failed>> {
        buf.fill(black_box(0));
        outcome((black_box(buf.len()), peer()))
    }
}

impl Readable for Datagrams {
    fn readable(&mut self) -> impl Future<Output = Result<(), Failed>> {
        outcome(())
    }
}

impl UdpSend for Datagrams {
    fn send(
        &mut self,
        remote: SocketAddr,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Failed>> {
        black_box((remote, data));
        outcome(())
    }
}

impl UdpSplit for Datagrams {
    type Receive<'a> = Datagrams;
    type Send<'a> = Datagrams;

    fn split(&mut self) -> (Datagrams, Datagrams) {
        (Datagrams, Datagrams)
    }
}
