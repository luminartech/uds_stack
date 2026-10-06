//! `DoIpTransport` over a socket-free `DiagnosticEntity`: the ISO 14229-5:2022 clause 11
//! mapping and the clause 8 prescribed close, driven through `UdsTransport` alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test harness: scripts are fixed and futures complete in bounded polls"
)]

#[allow(dead_code, reason = "each test binary uses part of the shared mock")]
mod support;

use simple_doip::LogicalAddress;
use simple_doip::service::{ConnectionId, DoIpResult};
use support::{Exhausted, MockEntity, TESTER, Tester, Wire, block_on, poll_once_and_drop};
use uds_on_ip::profile::bench_reloads;
use uds_on_ip::{DoIpTransport, Error};
use uds_services::{
    Address, AfterSend, Ai, Mtype, SResult, TaType, TransportEvent, UdsTransport,
};
use uds_session::{AddressExtension, TransportError};

const CONNECTION: ConnectionId = ConnectionId::new(0);

/// The tester's request, as the driver sees it.
fn request_ai() -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0E00),
        ta: Address(0x0001),
        ta_type: TaType::Physical,
    }
}

/// The entity's response to the tester, as the driver sends it.
fn response_ai() -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: Address(0x0001),
        ta: Address(0x0E00),
        ta_type: TaType::Physical,
    }
}

type Transport<const MCTS: usize = 1> = DoIpTransport<MockEntity<MCTS>, MCTS>;

fn transport(script: impl IntoIterator<Item = Tester>) -> Transport {
    DoIpTransport::new(MockEntity::new(script), bench_reloads())
}

/// The next event, with its data copied out so the buffer can go.
fn next(t: &mut Transport<1>) -> TransportEvent<'static> {
    let mut buffer = [0u8; 16];
    let event = block_on(t.next_event(&mut buffer, None)).unwrap();
    leak(event)
}

fn leak(event: TransportEvent<'_>) -> TransportEvent<'static> {
    match event {
        TransportEvent::DataInd { ai, data } => TransportEvent::DataInd {
            ai,
            data: data.to_vec().leak(),
        },
        TransportEvent::DataTooLong { ai, data, declared } => TransportEvent::DataTooLong {
            ai,
            data: data.to_vec().leak(),
            declared,
        },
        TransportEvent::Periodic { ai, pdid, data } => TransportEvent::Periodic {
            ai,
            pdid,
            data: data.to_vec().leak(),
        },
        TransportEvent::DataConf { ai, result } => TransportEvent::DataConf { ai, result },
        TransportEvent::Closed { expected } => TransportEvent::Closed { expected },
        TransportEvent::Deadline => TransportEvent::Deadline,
    }
}

fn respond(t: &mut Transport<1>, pdu: &[u8]) {
    block_on(t.t_data_req(response_ai(), pdu, AfterSend::Continue)).unwrap();
}

/// A tester that has sent `request`, already delivered to the driver.
fn indicated(request: &[u8]) -> Transport {
    indicated_by(request, |_| {})
}

/// [`indicated`], over an entity `configure` has prepared.
fn indicated_by(request: &[u8], configure: impl FnOnce(&mut MockEntity<1>)) -> Transport {
    let mut entity = MockEntity::new([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, request.to_vec()),
    ]);
    configure(&mut entity);
    let mut t = DoIpTransport::new(entity, bench_reloads());
    assert_eq!(
        next(&mut t),
        TransportEvent::DataInd {
            ai: request_ai(),
            data: request.to_vec().leak(),
        }
    );
    t
}

/// ISO 14229-5:2022 REQ 4.3 Table 4 and REQ 4.4 Table 5: `DoIP_Data.indication` is
/// `T_Data.ind`, with `DoIP_SA`, `DoIP_TA` and `DoIP_TAtype` as the request's addressing.
#[test]
fn an_indication_is_a_data_indication_with_the_testers_addressing() {
    indicated(&[0x22, 0xF1, 0x90]);
}

/// REQ 4.3 Table 4: `T_Data.req` is `DoIP_Data.request`, routed to the tester's
/// connection, and its `DoIP_Data.confirm` is `T_Data.conf` with the request's own
/// addressing (`UDSS_LLR_0060`: one confirmation for every request).
#[test]
fn a_response_is_sent_on_the_testers_connection_and_confirmed() {
    let mut t = indicated(&[0x22, 0xF1, 0x90]);
    respond(&mut t, &[0x62, 0xF1, 0x90, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(
        t.entity().wire,
        [Wire::Data(CONNECTION, vec![0x62, 0xF1, 0x90, 0x01])]
    );
}

/// REQ 7.9: the server initiates the close after sending the positive
/// `DiagnosticSessionControl` response. The close waits for the response's
/// confirmation, and is made before that confirmation reaches the driver, which
/// executes the session change on it.
#[test]
fn a_positive_session_response_closes_the_connection_after_its_confirmation() {
    let mut t = indicated(&[0x10, 0x03]);
    respond(&mut t, &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(
        t.entity().wire,
        [],
        "nothing is sent or closed at the request"
    );

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(
        t.entity().wire,
        [
            Wire::Data(CONNECTION, vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
}

/// REQ 7.11: so does a positive `ECUReset` response; and the tester, back as a new
/// connection after routing activation (REQ 7.10), is served without another close.
#[test]
fn a_positive_reset_response_closes_and_the_tester_returns_on_a_new_connection() {
    let mut t = transport([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x11, 0x01]),
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x3E, 0x00]),
    ]);
    next(&mut t);
    respond(&mut t, &[0x51, 0x01]);
    next(&mut t);
    assert_eq!(t.entity().wire.last(), Some(&Wire::Close(CONNECTION)));

    assert_eq!(
        next(&mut t),
        TransportEvent::DataInd {
            ai: request_ai(),
            data: &[0x3E, 0x00],
        }
    );
    respond(&mut t, &[0x7E, 0x00]);
    next(&mut t);
    assert_eq!(
        t.entity().wire[2..],
        [Wire::Data(CONNECTION, vec![0x7E, 0x00])],
        "an ordinary response closes nothing"
    );
}

/// REQ 7.9 keys the close on *sending* the positive response: one whose write fails
/// closes nothing, and its failure reaches the driver as `DoIP_ERROR`'s
/// `T_Result`.
#[test]
fn a_positive_response_that_fails_to_send_closes_nothing() {
    let mut t = indicated_by(&[0x10, 0x03], |entity| {
        entity.fail_next_write = Some(DoIpResult::Error);
    });
    respond(&mut t, &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(11)),
        }
    );
    assert_eq!(t.entity().wire, []);
}

/// Dropped while the prescribed close is still in progress — as the driver drops the
/// `next_event` that loses its race — the transport loses neither the close nor the
/// confirmation it holds back: the next call finishes both, once.
#[test]
fn a_close_dropped_unfinished_is_finished_by_the_next_call() {
    let mut t = indicated_by(&[0x10, 0x03], |entity| entity.close_yields = true);
    respond(&mut t, &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);

    let mut buffer = [0u8; 16];
    assert!(!poll_once_and_drop(t.next_event(&mut buffer, None)));
    assert_eq!(
        t.entity().wire.len(),
        1,
        "the response is sent, the close is not"
    );

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(t.entity().wire.last(), Some(&Wire::Close(CONNECTION)));
    assert_eq!(t.entity().wire.len(), 2);
}

/// A tester that closes its connection after the positive `DiagnosticSessionControl`
/// response was requested and before it was confirmed is in the flow REQ 7.9
/// prescribes: the close is expected. Its confirmation still arrives, and the
/// connection, already gone, is not closed again.
#[test]
fn a_tester_leaving_while_owed_the_prescribed_close_is_expected() {
    let mut t = indicated_by(&[0x10, 0x03], |entity| {
        entity.confirms_last = true;
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    respond(&mut t, &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);

    assert_eq!(next(&mut t), TransportEvent::Closed { expected: true });
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert!(!t.entity().wire.contains(&Wire::Close(CONNECTION)));
}

/// A tester that leaves during an ordinary exchange is not in any flow the standard
/// prescribes.
#[test]
fn a_tester_leaving_mid_service_is_an_unexpected_close() {
    let mut t = indicated_by(&[0x22, 0xF1, 0x90], |entity| {
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    assert_eq!(next(&mut t), TransportEvent::Closed { expected: false });
}

/// A response to a tester that has left is confirmed `DoIP_NO_SOCKET`, not refused,
/// so the driver still gets the one confirmation it waits on (`UDSS_LLR_0060`).
#[test]
fn a_response_to_a_tester_that_left_is_confirmed_no_socket() {
    let mut t = indicated_by(&[0x22, 0xF1, 0x90], |entity| {
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    next(&mut t);
    respond(&mut t, &[0x62, 0xF1, 0x90, 0x01]);
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(10)),
        }
    );
}

/// A request longer than the driver's buffer is `DataTooLong`, with the length
/// `DoIP`'s generic header declared, never a `DataInd` of its first bytes.
#[test]
fn a_request_longer_than_the_buffer_is_too_long_with_its_declared_length() {
    let mut t = transport([
        Tester::Connects(TESTER),
        Tester::Sends(TESTER, vec![0x2E, 0xF1, 0x90, 0xAA, 0xBB]),
    ]);
    let mut buffer = [0u8; 3];
    assert_eq!(
        block_on(t.next_event(&mut buffer, None)).unwrap(),
        TransportEvent::DataTooLong {
            ai: request_ai(),
            data: &[0x2E, 0xF1, 0x90],
            declared: Some(5),
        }
    );
}

/// A payload type ISO 14229-5 gives a server no use for — here REQ 7.16's periodic
/// response — is passed over, and the next event is the driver's.
#[test]
fn an_unmodelled_payload_is_passed_over() {
    let mut t = transport([
        Tester::Connects(TESTER),
        Tester::SendsUnmodelled(TESTER, 0x8004, vec![0x01, 0xAA]),
        Tester::Sends(TESTER, vec![0x3E, 0x00]),
    ]);
    assert_eq!(
        next(&mut t),
        TransportEvent::DataInd {
            ai: request_ai(),
            data: &[0x3E, 0x00],
        }
    );
}

/// An entity with more connections than the transport's table is a configuration
/// error, reported rather than leaving a tester's close unmade.
#[test]
fn a_connection_beyond_the_transports_table_is_an_error() {
    let other = LogicalAddress(0x0E80);
    let mut t: DoIpTransport<MockEntity<2>, 1> = DoIpTransport::new(
        MockEntity::new([
            Tester::Connects(other),
            Tester::Connects(TESTER),
            Tester::Sends(TESTER, vec![0x3E, 0x00]),
        ]),
        bench_reloads(),
    );
    let mut buffer = [0u8; 16];
    let error = block_on(t.next_event(&mut buffer, None)).unwrap_err();
    assert!(
        matches!(
            error,
            Error::ConnectionOutsideTable {
                connection,
                capacity: 1,
            } if connection == ConnectionId::new(1)
        ),
        "{error:?}"
    );
}

/// REQ 4.4 Table 5: `DoIP` has no address extension, so a remote message type is
/// refused before anything reaches the entity.
#[test]
fn a_remote_message_type_is_refused_before_the_entity() {
    let mut t = indicated(&[0x22, 0xF1, 0x90]);
    let remote = Ai {
        mtype: Mtype::RDiag {
            ae: AddressExtension(0x01),
        },
        ..response_ai()
    };
    let error = block_on(t.t_data_req(remote, &[0x62], AfterSend::Continue)).unwrap_err();
    assert!(matches!(error, Error::Mapping(_)), "{error:?}");
    let mut buffer = [0u8; 16];
    assert!(
        matches!(
            block_on(t.next_event(&mut buffer, None)),
            Err(Error::Entity(Exhausted))
        ),
        "no confirmation follows a refused request"
    );
}
