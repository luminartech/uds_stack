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

use simple_doip::service::{ConnectionId, DoIpResult};
use support::{Fault, MockEntity, TESTER, Tester, Wire, block_on, poll_once_and_drop};
use uds_on_ip::profile::bench_reloads;
use uds_on_ip::{DoIpTransport, Error};
use uds_services::{
    Address, AfterSend, Ai, Mtype, SResult, TaType, TransportEvent, UdsTransport,
};
use uds_session::{AddressExtension, Timestamp, TransportError};

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

type Transport<const CONNECTIONS: usize = 1> =
    DoIpTransport<MockEntity<CONNECTIONS>, CONNECTIONS>;

fn transport(script: impl IntoIterator<Item = Tester>) -> Transport {
    DoIpTransport::new(MockEntity::new(script), bench_reloads())
}

/// The next event, with its data copied out so the buffer can go.
fn next(t: &mut Transport<1>) -> Seen {
    let mut buffer = [0u8; 16];
    let event = block_on(t.next_event(&mut buffer, None)).unwrap();
    Seen::from(event)
}

/// A [`TransportEvent`] that owns its data, compared against one that borrows.
#[derive(Debug)]
enum Seen {
    DataInd {
        ai: Ai,
        data: Vec<u8>,
    },
    DataTooLong {
        ai: Ai,
        data: Vec<u8>,
        declared: Option<usize>,
    },
    Periodic {
        ai: Ai,
        pdid: u8,
        data: Vec<u8>,
    },
    Bare(TransportEvent<'static>),
}

impl From<TransportEvent<'_>> for Seen {
    fn from(event: TransportEvent<'_>) -> Self {
        match event {
            TransportEvent::DataInd { ai, data } => Self::DataInd {
                ai,
                data: data.to_vec(),
            },
            TransportEvent::DataTooLong { ai, data, declared } => Self::DataTooLong {
                ai,
                data: data.to_vec(),
                declared,
            },
            TransportEvent::Periodic { ai, pdid, data } => Self::Periodic {
                ai,
                pdid,
                data: data.to_vec(),
            },
            TransportEvent::DataConf { ai, result } => {
                Self::Bare(TransportEvent::DataConf { ai, result })
            }
            TransportEvent::Closed { peer, expected } => {
                Self::Bare(TransportEvent::Closed { peer, expected })
            }
            TransportEvent::Deadline => Self::Bare(TransportEvent::Deadline),
        }
    }
}

impl PartialEq<TransportEvent<'_>> for Seen {
    fn eq(&self, other: &TransportEvent<'_>) -> bool {
        match (self, other) {
            (Self::DataInd { ai, data }, TransportEvent::DataInd { ai: a, data: d }) => {
                ai == a && data == d
            }
            (
                Self::DataTooLong { ai, data, declared },
                TransportEvent::DataTooLong {
                    ai: a,
                    data: d,
                    declared: l,
                },
            ) => ai == a && data == d && declared == l,
            (
                Self::Periodic { ai, pdid, data },
                TransportEvent::Periodic {
                    ai: a,
                    pdid: p,
                    data: d,
                },
            ) => ai == a && pdid == p && data == d,
            (Self::Bare(seen), other) => seen == other,
            _ => false,
        }
    }
}

fn respond(t: &mut Transport<1>, pdu: &[u8]) {
    block_on(t.t_data_req(response_ai(), pdu, AfterSend::Continue)).unwrap();
}

/// [`respond`] with a message the server leaves its running software on.
fn respond_leaving(t: &mut Transport<1>, pdu: &[u8]) {
    block_on(t.t_data_req(response_ai(), pdu, AfterSend::ServerLeaves)).unwrap();
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
            data: request,
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

/// `UDSS_LLR_0060`: the confirmation carries the addressing the request was made
/// with, which the driver matches it by, though `DoIP` has no field for the message
/// type.
#[test]
fn a_confirmation_carries_the_addressing_it_was_requested_with() {
    let mut t = indicated(&[0x3E, 0x00]);
    let requested = Ai {
        mtype: Mtype::SecureDiag,
        ..response_ai()
    };
    block_on(t.t_data_req(requested, &[0x7E, 0x00], AfterSend::Continue)).unwrap();
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: requested,
            result: SResult::Ok,
        }
    );
}

/// REQ 4.4 Table 5 maps `T_SA` to `DoIP_SA`, so a response from an address the
/// entity does not own is not sent under the entity's, and its confirmation fails
/// with `DoIP_UNKNOWN_SA`, third in ISO 13400-2:2019 8.2.5's order.
#[test]
fn a_response_from_an_address_the_entity_does_not_own_is_not_sent() {
    let mut t = indicated(&[0x3E, 0x00]);
    let foreign = Ai {
        sa: Address(0x0002),
        ..response_ai()
    };
    block_on(t.t_data_req(foreign, &[0x7E, 0x00], AfterSend::Continue)).unwrap();
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: foreign,
            result: SResult::Transport(TransportError(3)),
        }
    );
    assert_eq!(t.entity().wire, []);
}

/// REQ 7.11: the server initiates the close after sending the positive `ECUReset`
/// response. The close waits for the response's confirmation, and is made before
/// that confirmation reaches the driver, which executes the reset on it.
#[test]
fn a_positive_reset_response_closes_the_connection_after_its_confirmation() {
    let mut t = indicated(&[0x11, 0x01]);
    respond(&mut t, &[0x51, 0x01]);
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
            Wire::Data(CONNECTION, vec![0x51, 0x01]),
            Wire::Close(CONNECTION),
        ]
    );
}

/// A busy refusal sent to the tester before the positive `ECUReset` response is
/// confirmed does not reorder REQ 7.11: the response's confirmation still reaches
/// the driver after the close, and the refusal, written before the close, is
/// confirmed after it.
#[test]
fn a_message_sent_after_the_reset_response_does_not_delay_its_close() {
    let mut t = indicated(&[0x11, 0x01]);
    respond(&mut t, &[0x51, 0x01]);
    respond(&mut t, &[0x7F, 0x22, 0x21]);

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
            Wire::Data(CONNECTION, vec![0x51, 0x01]),
            Wire::Data(CONNECTION, vec![0x7F, 0x22, 0x21]),
            Wire::Close(CONNECTION),
        ]
    );
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_nothing_follows(&mut t);
}

/// The tester, back as a new connection after routing activation (REQ 7.10), is
/// served without another close.
#[test]
fn after_the_prescribed_close_the_tester_returns_on_a_new_connection() {
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

/// REQ 7.11 keys the close on *sending* the positive response: one whose write fails
/// closes nothing, and its failure reaches the driver as `DoIP_ERROR`'s
/// `T_Result`.
#[test]
fn a_positive_response_that_fails_to_send_closes_nothing() {
    let mut t = indicated_by(&[0x11, 0x01], |entity| {
        entity.fail_next_write = Some(DoIpResult::Error);
    });
    respond(&mut t, &[0x51, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(11)),
        }
    );
    assert_eq!(t.entity().wire, []);
}

/// ISO 13400-2:2019 8.3.1 confirms every request, so a request the entity refuses is
/// confirmed failed rather than ending the server: nothing is sent, and a refused reset
/// response closes nothing.
#[test]
fn a_refused_request_is_confirmed_failed() {
    let mut t = indicated_by(&[0x11, 0x01], |entity| {
        entity.refuse.push_back(true);
    });
    respond(&mut t, &[0x51, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(11)),
        }
    );
    assert_eq!(t.entity().wire, []);
    assert_nothing_follows(&mut t);
}

/// `UDSS_LLR_0060` matches a confirmation to its request by order, so a refusal is
/// confirmed after the requests accepted before it, not ahead of them.
#[test]
fn a_refusal_is_confirmed_after_the_requests_accepted_before_it() {
    let mut t = indicated_by(&[0x22, 0xF1, 0x90], |entity| {
        entity.refuse.extend([false, true]);
    });
    respond(&mut t, &[0x7F, 0x22, 0x78]);
    respond(&mut t, &[0x62, 0xF1, 0x90, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(11)),
        }
    );
    assert_eq!(
        t.entity().wire,
        [Wire::Data(CONNECTION, vec![0x7F, 0x22, 0x78])]
    );
}

/// A refusal beyond those the transport can hold unreported is the entity's error.
#[test]
fn a_refusal_beyond_those_held_is_an_error() {
    let mut t = indicated_by(&[0x3E, 0x00], |entity| {
        entity.refuse.extend([true; 5]);
    });
    for _ in 0..4 {
        respond(&mut t, &[0x7E, 0x00]);
    }
    let error = block_on(t.t_data_req(response_ai(), &[0x7E, 0x00], AfterSend::Continue))
        .unwrap_err();
    assert!(matches!(error, Error::Entity(Fault::Refused)), "{error:?}");
}

/// Dropped while the prescribed close is still in progress — as the driver drops the
/// `next_event` that loses its race — the transport loses neither the close nor the
/// confirmation it holds back: the next call finishes both, once.
#[test]
fn a_close_dropped_unfinished_is_finished_by_the_next_call() {
    let mut t = indicated_by(&[0x11, 0x01], |entity| entity.close_yields = true);
    respond(&mut t, &[0x51, 0x01]);

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
    assert_nothing_follows(&mut t);
}

/// The script is spent: no event, a held-back confirmation least of all, is left.
fn assert_nothing_follows(t: &mut Transport<1>) {
    let mut buffer = [0u8; 16];
    assert!(matches!(
        block_on(t.next_event(&mut buffer, None)),
        Err(Error::Entity(Fault::Exhausted))
    ));
}

/// A prescribed close that fails has still ended the connection, so the
/// confirmation it held back is reported at once rather than behind an error that
/// ends the server: the driver executes the reset on it.
#[test]
fn a_close_that_fails_still_reports_the_confirmation_it_held_back() {
    let mut t = indicated_by(&[0x11, 0x01], |entity| entity.fail_next_close = true);
    respond(&mut t, &[0x51, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(t.entity().wire.last(), Some(&Wire::Close(CONNECTION)));
    assert_nothing_follows(&mut t);
}

/// A tester that closes its connection after the positive `ECUReset` response was
/// requested and before it was confirmed is in the flow REQ 7.11 prescribes: the
/// close is expected. Its confirmation still arrives, and the
/// connection, already gone, is not closed again.
#[test]
fn a_tester_leaving_while_owed_the_prescribed_close_is_expected() {
    let mut t = indicated_by(&[0x11, 0x01], |entity| {
        entity.confirms_last = true;
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    respond(&mut t, &[0x51, 0x01]);

    assert_eq!(
        next(&mut t),
        TransportEvent::Closed {
            peer: request_ai().sa,
            expected: true,
        }
    );
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert!(!t.entity().wire.contains(&Wire::Close(CONNECTION)));
}

/// ISO 14229-5:2022 REQ 7.9 closes after a positive `DiagnosticSessionControl`
/// response only where the session change disconnects, which the response's octets
/// cannot say: `50 03` the server stays on is sent and confirmed, and the connection
/// stays.
#[test]
fn a_positive_session_response_keeps_the_connection() {
    let mut t = indicated(&[0x10, 0x03]);
    respond(&mut t, &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]);
    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Ok,
        }
    );
    assert_eq!(
        t.entity().wire,
        [Wire::Data(
            CONNECTION,
            vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]
        )]
    );
}

/// REQ 7.9: a session change the server leaves its running software for is closed
/// as an `ECUReset` is — after the response's confirmation, before that confirmation
/// reaches the driver, which executes the change on it (ISO 14229-5:2022 Figure 5).
#[test]
fn a_response_the_server_leaves_on_closes_the_connection_after_its_confirmation() {
    let mut t = indicated(&[0x10, 0x02]);
    respond_leaving(&mut t, &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]);
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
            Wire::Data(CONNECTION, vec![0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
            Wire::Close(CONNECTION),
        ]
    );
}

/// The server leaves only once the response is sent: one whose write fails leaves
/// the server where it was, and closes nothing.
#[test]
fn a_response_the_server_leaves_on_that_fails_to_send_closes_nothing() {
    let mut t = indicated_by(&[0x10, 0x02], |entity| {
        entity.fail_next_write = Some(DoIpResult::Error);
    });
    respond_leaving(&mut t, &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]);

    assert_eq!(
        next(&mut t),
        TransportEvent::DataConf {
            ai: response_ai(),
            result: SResult::Transport(TransportError(11)),
        }
    );
    assert_eq!(t.entity().wire, []);
}

/// A tester that closes its connection while owed REQ 7.9's close is in the flow it
/// prescribes, as for `ECUReset`.
#[test]
fn a_tester_leaving_while_owed_the_session_close_is_expected() {
    let mut t = indicated_by(&[0x10, 0x02], |entity| {
        entity.confirms_last = true;
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    respond_leaving(&mut t, &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]);

    assert_eq!(
        next(&mut t),
        TransportEvent::Closed {
            peer: request_ai().sa,
            expected: true,
        }
    );
}

/// A tester that leaves during an ordinary exchange is not in any flow the standard
/// prescribes.
#[test]
fn a_tester_leaving_mid_service_is_an_unexpected_close() {
    let mut t = indicated_by(&[0x22, 0xF1, 0x90], |entity| {
        entity.script.push_back(Tester::Leaves(TESTER));
    });
    assert_eq!(
        next(&mut t),
        TransportEvent::Closed {
            peer: request_ai().sa,
            expected: false,
        }
    );
}

/// An entity reporting a request from outside the buffer it was lent is refused,
/// not trusted: the PDU is never handed to the driver.
#[test]
fn a_pdu_outside_the_lent_buffer_is_an_error() {
    let mut t = transport([Tester::Connects(TESTER), Tester::SendsOutsideBuffer(TESTER)]);
    let mut buffer = [0u8; 16];
    assert!(matches!(
        block_on(t.next_event(&mut buffer, None)),
        Err(Error::PduOutsideBuffer)
    ));
}

/// A connection whose tester never sent a diagnostic message closes unreported: no
/// tester is known to have spoken on it, so no exchange can be waiting on it, and a
/// `Closed` naming no peer would end someone else's.
#[test]
fn a_close_with_no_known_tester_is_not_reported() {
    let mut t = transport([Tester::Connects(TESTER), Tester::Leaves(TESTER)]);
    let mut buffer = [0u8; 16];
    assert!(matches!(
        block_on(t.next_event(&mut buffer, None)),
        Err(Error::Entity(Fault::Exhausted))
    ));
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
            Err(Error::Entity(Fault::Exhausted))
        ),
        "no confirmation follows a refused request"
    );
}

/// `now()` is the entity's clock, which the entity's deadlines are on.
#[test]
fn now_reads_the_entity_clock() {
    let mut entity = MockEntity::new([]);
    entity.clock = 0xFFFF_FFF0;
    let t: Transport = DoIpTransport::new(entity, bench_reloads());
    assert_eq!(t.now(), Timestamp(0xFFFF_FFF0));
}

/// The server's request limit reaches the entity, which refuses a longer request before
/// acknowledging it (ISO 13400-2:2019 REQ 7.DoIP-072; #41).
#[test]
fn the_request_limit_reaches_the_entity() {
    let mut t = transport([]);
    t.limit_requests(1_026);
    assert_eq!(t.entity().request_limit, Some(1_026));
}
