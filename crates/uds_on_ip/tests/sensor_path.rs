//! The sensor's path end to end: `uds_server!` → `DoIpTransport` → `simple_doip`'s
//! `Entity`, with `simple_doip`'s `Tester` at the other end of a loopback socket.
//!
//! `end_to_end.rs` runs the same server over a scripted entity; this runs it over the
//! one the sensor ships, so what the entity does on the wire is what is tested.

// Not under Miri, which has no sockets. The transport's own logic runs under Miri in
// `end_to_end.rs` and `transport.rs`, over the scripted entity.
#![cfg(not(miri))]
#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "test harness: a failed expectation is the test failing"
)]

mod loopback;

use core::future::Future;

use loopback::{
    ENTITY, FUNCTIONAL, Loopback, SensorEntity, Then, advance, answered, ask, exchange,
    next, on_loopback, serve_until,
};
use simple_doip::TaType;
use simple_doip::messages::RoutingActivationResponseCode;
use simple_doip::service::TesterConnection;
use simple_doip::tester::ConnectError;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_protocol::NegativeResponseCode as Nrc;
use uds_services::{
    Access, Address, DiagnosticSessionControl, DiagnosticSessionType as S, EcuReset,
    ResetType, ResponseSink, ServerParams, SessionTiming, SessionTransition, Sessions,
    TesterPresent, uds_server,
};

/// The sensor's message size.
const MAX_MESSAGE: usize = 4096;

/// The sensor's transport: the entity serves one tester, so its table is two.
type Transport = DoIpTransport<SensorEntity<MAX_MESSAGE>, 2>;

/// An application that jumps to its bootloader for the programming session, and so
/// leaves its running software on that change (ISO 14229-1:2020 10.2.2.2 Table 25).
#[derive(Debug, Default)]
struct Ecu;

impl DiagnosticSessionControl for Ecu {
    const MAX_RESPONSE_LEN: usize = 0;
    fn supports(&self, s: S) -> bool {
        matches!(
            s,
            S::DefaultSession | S::ProgrammingSession | S::ExtendedDiagnosticSession
        )
    }
    /// Programming is entered only from Extended; every other session from any.
    fn supported_from(&self, s: S, active: S) -> bool {
        !matches!(s, S::ProgrammingSession)
            || matches!(active, S::ExtendedDiagnosticSession)
    }
    fn leaves_running_software(&self, s: S) -> bool {
        matches!(s, S::ProgrammingSession)
    }
    fn timing(&self, _s: S) -> SessionTiming {
        SessionTiming {
            p2_server_max_ms: 50,
            p2_star_server_max_10ms: 500,
        }
    }
    fn on_transition(&mut self, _t: SessionTransition, _entered: S, _relocked: bool) {}
}

impl EcuReset for Ecu {
    const MAY_RESPOND_PENDING: bool = false;
    fn access(&self, _kind: ResetType) -> Option<Access> {
        Some(Access::new(Sessions::ALL))
    }
    fn reset(
        &mut self,
        _kind: ResetType,
        _out: &mut ResponseSink<'_>,
    ) -> impl Future<Output = Result<(), Nrc>> {
        core::future::ready(Ok(()))
    }
}

impl TesterPresent for Ecu {
    fn on_tester_present(&mut self) {}
}

uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent;
    transport = Transport,
    peers = 1,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

fn server(loopback: &Loopback) -> EcuServer {
    EcuServer::new(
        Ecu,
        DoIpTransport::new(loopback.entity(), bench_reloads()),
        Address(ENTITY.0),
        PARAMS,
    )
}

/// A request is indicated, answered on the connection it arrived on, and the answer
/// confirmed; physically and functionally addressed alike (ISO 14229-5:2022 REQ 4.3
/// Table 4; ISO 13400-2:2019 8.3.3).
#[test]
fn a_request_is_answered() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut tester, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            assert_eq!(
                exchange(&mut tester, FUNCTIONAL, TaType::Functional, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A positive `ECUReset` response is followed by the entity closing the connection
/// (ISO 14229-5:2022 REQ 7.11), and the tester then reconnects and activates routing
/// again (REQ 7.10), on which it is served as before.
#[test]
fn a_reset_is_answered_then_closed_and_the_tester_reconnects() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut tester, &[0x11, 0x01]).await,
                answered(&[0x51, 0x01])
            );
            assert_eq!(next(&mut tester).await, Then::Closed);

            tester.reconnect().await.expect("reconnected");
            assert_eq!(
                ask(&mut tester, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A positive `DiagnosticSessionControl` response to a session change that leaves the
/// running software is followed by the entity closing the connection (ISO 14229-5:2022
/// REQ 7.9); a change that does not leave it closes nothing. The tester reconnects
/// (REQ 7.8).
#[test]
fn a_session_change_that_leaves_the_software_is_answered_then_closed() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut tester = loopback.tester().await.expect("activated");
            let extended = ask(&mut tester, &[0x10, 0x03]).await;
            assert!(
                matches!(&extended.then, Then::Response(_, pdu) if pdu.starts_with(&[0x50, 0x03])),
                "{extended:?}"
            );
            let programming = ask(&mut tester, &[0x10, 0x02]).await;
            assert!(
                matches!(&programming.then, Then::Response(_, pdu) if pdu.starts_with(&[0x50, 0x02])),
                "{programming:?}"
            );
            assert_eq!(next(&mut tester).await, Then::Closed);

            tester.reconnect().await.expect("reconnected");
            assert_eq!(ask(&mut tester, &[0x3E, 0x00]).await, answered(&[0x7E, 0x00]));
            tester.close().await.expect("closed");
        })
        .await;
    });
}

/// A second connection activating routing for the tester's address, while the first
/// still answers the alive check, is refused with `0x03` (ISO 13400-2:2019 Table 48,
/// REQ 3.DoIP-091). The first is served as before.
#[test]
fn a_second_connection_from_the_tester_is_refused_while_the_first_answers() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut first = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );

            let second = tokio::select! {
                second = loopback.tester() => second,
                then = next(&mut first) => panic!("the first saw {then:?}"),
            };
            assert!(
                matches!(
                    second,
                    Err(ConnectError::RoutingActivationDenied(
                        RoutingActivationResponseCode::DeniedSourceAddressAlreadyRegistered
                    ))
                ),
                "{second:?}"
            );
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            first.close().await.expect("closed");
        })
        .await;
    });
}

/// A first connection that does not answer the alive check within
/// `T_TCP_Alive_Check` is closed, and the second takes its address (ISO 13400-2:2019
/// REQ 3.DoIP-093). The transport then routes to the second, and an `ECUReset` there
/// closes the second (ISO 14229-5:2022 REQ 7.11).
#[test]
fn a_silent_first_connection_gives_way_to_the_second() {
    on_loopback(|loopback| async move {
        let mut server = server(&loopback);
        serve_until(&mut server, async {
            let mut first = loopback.tester().await.expect("activated");
            assert_eq!(
                ask(&mut first, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );

            let mut second = tokio::select! {
                second = loopback.tester() => second.expect("activated"),
                () = async {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        advance(embassy_time::Duration::from_millis(50));
                    }
                } => unreachable!(),
            };
            assert_eq!(
                ask(&mut second, &[0x3E, 0x00]).await,
                answered(&[0x7E, 0x00])
            );
            assert_eq!(next(&mut first).await, Then::Closed);

            assert_eq!(
                ask(&mut second, &[0x11, 0x01]).await,
                answered(&[0x51, 0x01])
            );
            assert_eq!(next(&mut second).await, Then::Closed);
        })
        .await;
    });
}
