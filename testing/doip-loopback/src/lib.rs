//! The `DoIP` path over loopback: `simple_doip`'s `Entity` serving a `uds_server!` server
//! through `uds_on_ip`'s `DoIpTransport`, and `simple_doip`'s `Tester` at the other end
//! of an `edge-nal-std` socket.
//!
//! A harness for any test that needs the real entity and tester rather than the scripted
//! mocks in `uds_on_ip`'s and `simple_doip`'s own tests: this crate's `sensor_path`
//! test, its `discovery` test, and the client transport's and the embedded probe's after
//! it. Unpublished, so that no published crate carries a feature or dependency for it.
//!
//! Write the test as a plain `#[test]` that calls [`on_loopback`]: it
//! takes the clock, binds a port, and runs the test's future on a current-thread
//! runtime within [`PATIENCE`]. Inside, build the entity with [`Loopback::entity`],
//! connect testers with [`Loopback::tester`], and race the server against the testers'
//! script with [`serve_until`].
//!
//! Timers run on `embassy-time`'s mock clock, which moves only when a test calls
//! [`advance`]. The clock is process-wide, so [`on_loopback`] holds it for the whole
//! test, and the tests in one binary run one at a time.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "a test harness: a failed expectation is the test failing"
)]

use core::future::Future;
use core::pin::Pin;
use std::net::SocketAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use edge_nal::{TcpBind, UdpBind};
use edge_nal_std::{Stack, TcpAcceptor, UdpSocket};
use embassy_time::MockDriver;
use simple_doip::entity::{Entity, EntityAddress};
use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, DoIpResult, EntityConfig, TesterAddress,
};
use simple_doip::tester::{ConnectError, Tester};
use simple_doip::{LogicalAddress, TaType};
use uds_services::{Server, ServiceSet, UdsTransport};

/// The entity's physical address.
pub const ENTITY: LogicalAddress = LogicalAddress(0x0001);
/// The functional group address the entity also answers.
pub const FUNCTIONAL: LogicalAddress = LogicalAddress(0xE400);
/// The one tester the sensor accepts.
pub const TESTER: LogicalAddress = LogicalAddress(0x0E00);

/// A real-time bound on a whole test. The mock clock does not move by itself, so a lost
/// message would otherwise hang.
pub const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// The largest message a [`SensorTester`] sends or receives, generic header included.
pub const TESTER_MESSAGE: usize = 4096;

/// The sensor's entity: one tester connection at a time (`MCTS` 1), so a connection
/// table of two, the reserve socket counted, and messages of up to `MAX_MESSAGE` bytes.
pub type SensorEntity<const MAX_MESSAGE: usize> =
    Entity<'static, TcpAcceptor, 1, MAX_MESSAGE>;

/// A tester on the loopback stack.
pub type SensorTester = Tester<'static, Stack, TESTER_MESSAGE>;

/// Why a tester could not connect.
pub type TesterConnectError = ConnectError<std::io::Error>;

static CLOCK: Mutex<()> = Mutex::new(());

/// Exclusive use of the process-wide mock clock, reset to zero.
fn clock() -> MutexGuard<'static, ()> {
    let guard = CLOCK.lock().unwrap_or_else(PoisonError::into_inner);
    MockDriver::get().reset();
    guard
}

/// Moves the mock clock forward, firing every timer that comes due.
pub fn advance(by: embassy_time::Duration) {
    MockDriver::get().advance(by);
}

/// Runs `test` against a freshly bound [`Loopback`], holding the mock clock, on a
/// current-thread runtime, within [`PATIENCE`].
///
/// # Panics
///
/// If the runtime cannot be built, the port cannot be bound, or `test` overruns
/// [`PATIENCE`].
pub fn on_loopback<F, Fut>(test: F)
where
    F: FnOnce(Loopback) -> Fut,
    Fut: Future<Output = ()>,
{
    let _clock = clock();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current-thread runtime");
    runtime.block_on(async {
        let loopback = Loopback::bind().await;
        tokio::time::timeout(PATIENCE, test(loopback))
            .await
            .expect("the test overran its patience");
    });
}

/// A bound loopback port, and the stack that dials it.
///
/// The stack and the acceptor are leaked, because the entity and the testers borrow
/// them for `'static`; a test binary leaks one of each per test.
#[derive(Clone, Copy)]
pub struct Loopback {
    stack: &'static Stack,
    acceptor: &'static TcpAcceptor,
    remote: SocketAddr,
}

impl core::fmt::Debug for Loopback {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Loopback")
            .field("remote", &self.remote)
            .finish_non_exhaustive()
    }
}

impl Loopback {
    /// Binds a free loopback port. `edge-nal-std` cannot report the port it bound, so
    /// one is found with a `std` listener first and bound again; another process can
    /// take it in between, so a few ports are tried.
    async fn bind() -> Self {
        const ATTEMPTS: usize = 8;
        let stack: &'static Stack = Box::leak(Box::new(Stack::new()));
        for _ in 0..ATTEMPTS {
            let remote = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|listener| listener.local_addr())
                .expect("a free loopback port");
            if let Ok(acceptor) = TcpBind::bind(stack, remote).await {
                return Self {
                    stack,
                    acceptor: Box::leak(Box::new(acceptor)),
                    remote,
                };
            }
        }
        panic!("no loopback port could be bound in {ATTEMPTS} attempts")
    }

    /// The sensor's entity on this port, answering [`ENTITY`] and [`FUNCTIONAL`] and
    /// accepting routing activation from [`TESTER`] only.
    ///
    /// # Panics
    ///
    /// Never: [`ENTITY`], [`FUNCTIONAL`] and [`TESTER`] are valid as what they are.
    #[must_use]
    pub fn entity<const MAX_MESSAGE: usize>(&self) -> SensorEntity<MAX_MESSAGE> {
        let address = EntityAddress::new(ENTITY, FUNCTIONAL).expect("entity addresses");
        let tester = TesterAddress::new(TESTER).expect("a tester address");
        Entity::new(self.acceptor, address, EntityConfig::new([tester]))
    }

    /// A UDP socket on a free loopback port, able to broadcast, and its address.
    ///
    /// # Panics
    ///
    /// If no port can be bound.
    pub async fn udp(&self) -> (UdpSocket, SocketAddr) {
        let any_port = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
        let socket = UdpBind::bind(self.stack, any_port)
            .await
            .expect("a loopback UDP port");
        let local = socket.get_ref().local_addr().expect("its address");
        (socket, local)
    }

    /// The address the entity takes `TCP_DATA` connections on: a free port, not
    /// [`simple_doip::TCP_PORT`].
    #[must_use]
    pub const fn tcp_address(&self) -> SocketAddr {
        self.remote
    }

    /// A tester connected as [`TESTER`], with routing activated. It reconnects with no
    /// back-off: the entity holds no address after a socket closes.
    ///
    /// # Errors
    ///
    /// Whatever [`Tester::connect`] returns.
    ///
    /// # Panics
    ///
    /// Never: [`TESTER`] is a tester's address.
    pub async fn tester(&self) -> Result<SensorTester, TesterConnectError> {
        let sa = TesterAddress::new(TESTER).expect("a tester address");
        Ok(Tester::connect(self.stack, self.remote, sa)
            .await?
            .with_reconnect_backoff(embassy_time::Duration::from_ticks(0)))
    }
}

/// Runs `server` until `client` completes, and returns what `client` returned.
///
/// Call it once per test, with the testers' whole script in `client`: the server is
/// stopped by dropping it mid-step, which `uds_services::Server::step` does not survive
/// (`luminartech/uds_stack#19`). The race is boxed, because the server's and the
/// testers' buffers make it tens of kilobytes.
///
/// # Panics
///
/// If the server fails.
pub fn serve_until<'s, A, T, const PEERS: usize, C>(
    server: &'s mut Server<A, T, PEERS>,
    client: C,
) -> Pin<Box<impl Future<Output = C::Output> + 's>>
where
    A: ServiceSet,
    T: UdsTransport,
    C: Future + 's,
{
    Box::pin(async move {
        tokio::select! {
            biased;
            output = client => output,
            failed = server.run() => panic!("the server stopped: {failed:?}"),
        }
    })
}

/// What a tester saw after one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    /// The request's `DoIP_Data.confirm` outcome: the entity's acknowledgement.
    pub confirmed: Option<DoIpResult>,
    /// What followed it.
    pub then: Then,
}

/// What followed a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// A response, from its source address.
    Response(LogicalAddress, Vec<u8>),
    /// The connection closed.
    Closed,
    /// Nothing: the request was not acknowledged, so no response follows it.
    Refused,
}

/// Sends `pdu` to `ta`, then waits for the first response or close, or returns at once if
/// the request is not acknowledged.
///
/// # Panics
///
/// If the request is refused, the tester fails, or it reports anything but a confirm, a
/// response or a close.
pub async fn exchange<C: DiagnosticConnection>(
    tester: &mut C,
    ta: LogicalAddress,
    ta_type: TaType,
    pdu: &[u8],
) -> Exchange {
    tester
        .request(ta, ta_type, pdu)
        .await
        .expect("the tester accepts the request");
    let mut confirmed = None;
    loop {
        let mut buf = vec![0u8; TESTER_MESSAGE];
        match tester.next_event(&mut buf, None).await.expect("the tester") {
            ConnectionEvent::Confirm { result, .. } if result != DoIpResult::Ok => {
                return Exchange {
                    confirmed: Some(result),
                    then: Then::Refused,
                };
            }
            ConnectionEvent::Confirm { result, .. } => confirmed = Some(result),
            ConnectionEvent::Indication { sa, pdu, .. } => {
                return Exchange {
                    confirmed,
                    then: Then::Response(sa, pdu.to_vec()),
                };
            }
            ConnectionEvent::Closed => {
                return Exchange {
                    confirmed,
                    then: Then::Closed,
                };
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Sends `pdu` physically to [`ENTITY`] and waits for its confirm alone: the entity's
/// acknowledgement, or its refusal.
///
/// # Panics
///
/// If the request is refused, the tester fails, or its next event is not the confirm.
pub async fn send<C: DiagnosticConnection>(tester: &mut C, pdu: &[u8]) -> DoIpResult {
    tester
        .request(ENTITY, TaType::Physical, pdu)
        .await
        .expect("the tester accepts the request");
    let mut buf = vec![0u8; TESTER_MESSAGE];
    match tester.next_event(&mut buf, None).await.expect("the tester") {
        ConnectionEvent::Confirm { result, .. } => result,
        other => panic!("unexpected {other:?}"),
    }
}

/// Sends `pdu` physically to [`ENTITY`]; see [`exchange`].
pub async fn ask<C: DiagnosticConnection>(tester: &mut C, pdu: &[u8]) -> Exchange {
    exchange(tester, ENTITY, TaType::Physical, pdu).await
}

/// The positive response `pdu` from [`ENTITY`], acknowledged.
#[must_use]
pub fn answered(pdu: &[u8]) -> Exchange {
    Exchange {
        confirmed: Some(DoIpResult::Ok),
        then: Then::Response(ENTITY, pdu.to_vec()),
    }
}

/// Waits for the next event a tester reports without sending anything first.
///
/// # Panics
///
/// If the tester fails.
pub async fn next<C: DiagnosticConnection>(tester: &mut C) -> Then {
    loop {
        let mut buf = vec![0u8; TESTER_MESSAGE];
        match tester.next_event(&mut buf, None).await.expect("the tester") {
            ConnectionEvent::Indication { sa, pdu, .. } => {
                return Then::Response(sa, pdu.to_vec());
            }
            ConnectionEvent::Closed => return Then::Closed,
            ConnectionEvent::Confirm { .. }
            | ConnectionEvent::IndicationTruncated { .. }
            | ConnectionEvent::Unmodelled { .. }
            | ConnectionEvent::UnmodelledTruncated { .. }
            | ConnectionEvent::Deadline => {}
        }
    }
}
