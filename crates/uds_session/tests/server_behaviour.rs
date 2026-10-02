//! Behavioural tests of the server role, one per requirement a body satisfies.

use uds_session::{
    Address, Ai, Association, Mtype, PeerIdentity, Rejection, SResult, Server,
    ServerOutput, ServerParams, ServerReaction, ServerReload, ServerRx, ServerTx,
    SessionSelection, Solicitation, TaType, Timestamp,
};

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

/// [`PARAMS`] with a response-pending lead of 10 ms (``UDSS_LLR_0186``).
const PARAMS_LEAD: ServerParams = ServerParams {
    response_pending_lead: 10,
    ..PARAMS
};

fn server() -> Server<2> {
    Server::new([Association::EMPTY, Association::EMPTY], PARAMS)
}

fn ai(sa: u16, ta: u16) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: Address(sa),
        ta: Address(ta),
        ta_type: TaType::Physical,
    }
}

const TESTER: u16 = 0x0E80;
const OTHER_TESTER: u16 = 0x0E81;
const ECU: u16 = 0x0010;

fn peer(address: u16) -> PeerIdentity {
    PeerIdentity {
        address: Address(address),
        extension: None,
    }
}

/// Drain a reaction into a fixed array (no alloc), then finish it.
fn outputs<'d>(
    mut r: ServerReaction<'_, 'd, 2>,
) -> ([Option<ServerOutput<'d>>; 4], Result<(), Rejection>) {
    let mut out = [None, None, None, None];
    for (slot, o) in out.iter_mut().zip(r.outputs()) {
        *slot = Some(o);
    }
    (out, r.finish())
}

/// Put the server in a non-default session controlled by `TESTER`, by the path
/// ``UDSS_LLR_0085`` prescribes: a solicited positive response selecting it, confirmed.
fn enter_non_default(s: &mut Server<2>, now: Timestamp) {
    let req = ai(TESTER, ECU);
    let rsp = ai(ECU, TESTER);
    let (_, ok) = outputs(s.t_data_ind(
        now,
        req,
        &[0x10, 0x03],
        SResult::Ok,
        ServerRx::Request { session: None },
    ));
    assert!(ok.is_ok());
    let (_, ok) = outputs(s.s_data_req(
        now,
        rsp,
        &[0x50, 0x03, 0, 50, 1, 244],
        ServerTx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: Some(SessionSelection::NonDefault),
        },
    ));
    assert!(ok.is_ok());
    let (_, ok) = outputs(s.t_data_conf(now, rsp, SResult::Ok));
    assert!(ok.is_ok());
}

mod expiry {
    use super::*;

    /// ``UDSS_LLR_0083``, ``UDSS_LLR_0102`` — nothing runs on a fresh server.
    #[test]
    fn a_fresh_server_has_no_deadline() {
        assert_eq!(server().next_deadline(), None);
    }

    /// ``UDSS_LLR_0100`` — at `tS3_Server` the session ends, the timer is disabled, the
    /// controlling client is discarded, and the indication names that client.
    #[test]
    fn session_timeout_returns_to_default_and_names_the_client() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_000)));
        let (out, ok) = outputs(s.tick(Timestamp(5_000)));
        assert!(ok.is_ok());
        assert_eq!(
            out[0],
            Some(ServerOutput::SessionTimeout {
                client: peer(TESTER)
            })
        );
        assert_eq!(out[1], None);
        assert_eq!(s.next_deadline(), None);
        // A second tick reports nothing: the snapshot was taken once (UDSS_LLR_0117's
        // "one overrun yields one indication" reasoning, applied here too).
        let (out, _) = outputs(s.tick(Timestamp(5_001)));
        assert_eq!(out[0], None);
    }

    /// ``UDSS_LLR_0081`` — the expiry is reported before the input's own output, and the
    /// input is processed in the state the expiry left: the request arriving at the
    /// timeout is answered in the default session.
    #[test]
    fn an_expiry_precedes_the_input_that_carried_the_timestamp() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (out, ok) = outputs(s.t_data_ind(
            Timestamp(5_000),
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
        assert!(matches!(out[0], Some(ServerOutput::SessionTimeout { .. })));
        assert!(matches!(out[1], Some(ServerOutput::Indicate { .. })));
        // tS3_Server is not running: the server is in the default session (UDSS_LLR_0099).
        // tP2_Server is: the request started it (UDSS_LLR_0113).
        assert_eq!(s.next_deadline(), Some(Timestamp(5_050)));
    }

    /// ``UDSS_LLR_0117`` — at `tP2_Server` the timer stops and the indication names the
    /// service in progress and the parameter the timer carried.
    #[test]
    fn response_overrun_names_the_service_and_the_reload() {
        let mut s = server();
        let (_, ok) = outputs(s.t_data_ind(
            Timestamp(0),
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), Some(Timestamp(50)));
        let (out, _) = outputs(s.tick(Timestamp(50)));
        assert_eq!(
            out[0],
            Some(ServerOutput::ResponseOverrun {
                sa: Address(TESTER),
                ae: None,
                loaded: ServerReload::P2,
            })
        );
        assert_eq!(s.next_deadline(), None);
    }

    /// ``UDSS_LLR_0019`` — a session timer started near the wrap expires past it, and the
    /// deadline is reported as the wrapped value.
    #[test]
    fn the_session_timer_runs_across_the_wrap() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(u32::MAX - 999));
        assert_eq!(s.next_deadline(), Some(Timestamp(4_000)));
        let (out, _) = outputs(s.tick(Timestamp(3_999)));
        assert_eq!(out[0], None);
        let (out, _) = outputs(s.tick(Timestamp(4_000)));
        assert!(matches!(out[0], Some(ServerOutput::SessionTimeout { .. })));
    }

    /// ``UDSS_LLR_0081`` — both timers expiring at one timestamp are both acted on before
    /// the input, and the drain yields the session timeout first. Both run at once only
    /// for a service another client began (``UDSS_LLR_0097``): the controlling client's
    /// own request stops `tS3_Server` (``UDSS_LLR_0087``).
    #[test]
    fn both_timers_expiring_on_one_tick_report_session_timeout_first() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0)); // tS3 due at 5000
        let (_, ok) = outputs(s.t_data_ind(
            Timestamp(4_950),
            ai(OTHER_TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        )); // tP2 due at 5000
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), Some(Timestamp(5_000)));
        let (out, ok) = outputs(s.tick(Timestamp(5_000)));
        assert!(ok.is_ok());
        assert_eq!(
            out,
            [
                Some(ServerOutput::SessionTimeout {
                    client: peer(TESTER)
                }),
                Some(ServerOutput::ResponseOverrun {
                    sa: Address(OTHER_TESTER),
                    ae: None,
                    loaded: ServerReload::P2,
                }),
                None,
                None,
            ]
        );
        assert_eq!(s.next_deadline(), None);
    }
}

mod indication {
    use super::*;

    /// ``UDSS_LLR_0036`` — both outcomes of a reception are indicated.
    #[test]
    fn every_reception_is_indicated() {
        let mut s = server();
        let data = [0x22, 0xF1, 0x90];
        let (out, ok) = outputs(s.t_data_ind(
            Timestamp(0),
            ai(TESTER, ECU),
            &data,
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
        assert_eq!(
            out[0],
            Some(ServerOutput::Indicate {
                ai: ai(TESTER, ECU),
                data: &data,
                result: SResult::Ok,
            })
        );
        let failed = SResult::Transport(uds_session::TransportError(1));
        let (out, _) = outputs(s.t_data_ind(
            Timestamp(1),
            ai(TESTER, ECU),
            &[],
            failed,
            ServerRx::Request { session: None },
        ));
        assert!(matches!(
            out[0],
            Some(ServerOutput::Indicate { result, .. }) if result == failed
        ));
    }

    /// ``UDSS_LLR_0113`` — a successful request starts `tP2_Server` with `tP2_Server_Max`;
    /// ``UDSS_LLR_0099`` — in the default session no request starts `tS3_Server`.
    #[test]
    fn a_request_starts_the_response_timer_only() {
        let mut s = server();
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(100),
            ai(TESTER, ECU),
            &[0x3E, 0x00],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(150)));
    }

    /// ``UDSS_LLR_0087`` — a request from the controlling client stops `tS3_Server`.
    #[test]
    fn a_request_from_the_controlling_client_stops_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(1_000),
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        // Only tP2_Server is running now.
        assert_eq!(s.next_deadline(), Some(Timestamp(1_050)));
    }

    /// ``UDSS_LLR_0097`` — a request from another client leaves `tS3_Server` alone.
    #[test]
    fn a_request_from_another_client_does_not_touch_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(1_000),
            ai(OTHER_TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        // tP2 at 1050 would be earlier than tS3 at 5000 — so check tS3 directly by ticking
        // past tP2 and seeing the session still times out at 5000.
        let (_, _) = outputs(s.tick(Timestamp(1_050)));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_000)));
    }

    /// ``UDSS_LLR_0095`` — a keep-alive from the controlling client reloads a running
    /// `tS3_Server`; ``UDSS_LLR_0096`` — one from another client changes nothing.
    #[test]
    fn a_keep_alive_reloads_the_running_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(2_000),
            ai(TESTER, ECU),
            &[0x3E, 0x80],
            SResult::Ok,
            ServerRx::KeepAlive,
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(7_000)));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(3_000),
            ai(OTHER_TESTER, ECU),
            &[0x3E, 0x80],
            SResult::Ok,
            ServerRx::KeepAlive,
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(7_000)));
    }

    /// ``UDSS_LLR_0092`` — a failed reception from the controlling client, while the
    /// timer is stopped and no service is in progress, restarts `tS3_Server`.
    #[test]
    fn a_reception_error_restarts_a_stopped_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        // Stop the timer with a start-of-message, which begins no service (UDSS_LLR_0107).
        let (_, _) = outputs(s.t_data_som_ind(
            Timestamp(1_000),
            ai(TESTER, ECU),
            ServerRx::Request { session: None },
        ));
        assert_eq!(s.next_deadline(), None);
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(1_100),
            ai(TESTER, ECU),
            &[],
            SResult::Transport(uds_session::TransportError(2)),
            ServerRx::Request { session: None },
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_100)));
    }

    /// ``UDSS_LLR_0043``, ``UDSS_LLR_0076`` — a parameter change affects only timers
    /// started afterwards.
    #[test]
    fn a_parameter_change_does_not_move_a_running_timer() {
        let mut s = server();
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(0),
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        let (_, ok) = outputs(s.set_parameter(
            Timestamp(10),
            uds_session::ServerParameter::P2ServerMax(500),
        ));
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), Some(Timestamp(50)));
        let (_, _) = outputs(s.tick(Timestamp(50)));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(60),
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(560)));
    }
}

mod transmission {
    use super::*;
    use uds_session::{Cause, TransportError};

    fn request_in_progress(s: &mut Server<2>, now: Timestamp) {
        let (_, ok) = outputs(s.t_data_ind(
            now,
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
    }

    const FINAL: ServerTx = ServerTx::FinalResponse {
        solicitation: Solicitation::Solicited,
        session: None,
    };

    /// ``UDSS_LLR_0033``, ``UDSS_LLR_0024`` — a request becomes a `Transmit` of the same
    /// bytes to the same addressing.
    #[test]
    fn a_request_is_transmitted_as_given() {
        let mut s = server();
        request_in_progress(&mut s, Timestamp(0));
        let data = [0x62, 0xF1, 0x90, 0x01];
        let (out, ok) = outputs(s.s_data_req(Timestamp(5), ai(ECU, TESTER), &data, FINAL));
        assert!(ok.is_ok());
        assert_eq!(
            out[0],
            Some(ServerOutput::Transmit {
                ai: ai(ECU, TESTER),
                data: &data,
            })
        );
    }

    /// ``UDSS_LLR_0114`` — passing the solicited final response to the transport stops
    /// `tP2_Server`.
    #[test]
    fn a_final_response_stops_the_response_timer() {
        let mut s = server();
        request_in_progress(&mut s, Timestamp(0));
        let (_, _) = outputs(s.s_data_req(Timestamp(5), ai(ECU, TESTER), &[0x62], FINAL));
        assert_eq!(s.next_deadline(), None);
    }

    /// ``UDSS_LLR_0061`` — a request duplicating an outstanding association is rejected;
    /// ``UDSS_LLR_0015`` — and changes nothing: the first association is still matched.
    #[test]
    fn a_duplicate_outstanding_request_is_rejected() {
        let mut s = server();
        request_in_progress(&mut s, Timestamp(0));
        let (_, ok) = outputs(s.s_data_req(Timestamp(5), ai(ECU, TESTER), &[0x62], FINAL));
        assert!(ok.is_ok());
        let (out, err) =
            outputs(s.s_data_req(Timestamp(6), ai(ECU, TESTER), &[0x62], FINAL));
        assert_eq!(out[0], None);
        assert!(err.is_err_and(|r| r.contains(Cause::AssociationOutstanding)));
        let (_, ok) = outputs(s.t_data_conf(Timestamp(7), ai(ECU, TESTER), SResult::Ok));
        assert!(ok.is_ok());
    }

    /// ``UDSS_LLR_0062`` — no free association rejects; a server sized for two peers
    /// refuses the third outstanding transmission.
    #[test]
    fn no_free_association_is_rejected() {
        let mut s = server();
        let (_, a) = outputs(s.s_data_req(Timestamp(0), ai(ECU, 0x0E80), &[0x62], FINAL));
        let (_, b) = outputs(s.s_data_req(Timestamp(0), ai(ECU, 0x0E81), &[0x62], FINAL));
        let (_, c) = outputs(s.s_data_req(Timestamp(0), ai(ECU, 0x0E82), &[0x62], FINAL));
        assert!(a.is_ok() && b.is_ok());
        assert!(c.is_err_and(|r| r.contains(Cause::NoAssociationFree)));
    }

    /// ``UDSS_LLR_0063`` — a confirmation matching no association is rejected and changes
    /// nothing.
    #[test]
    fn an_unmatched_confirmation_is_rejected() {
        let mut s = server();
        let (out, err) = outputs(s.t_data_conf(Timestamp(0), ai(ECU, TESTER), SResult::Ok));
        assert_eq!(out[0], None);
        assert!(err.is_err_and(|r| r.contains(Cause::NoMatchingAssociation)));
        assert_eq!(s.next_deadline(), None);
    }

    /// ``UDSS_LLR_0037``, ``UDSS_LLR_0039``, ``UDSS_LLR_0059`` — the confirmation is
    /// matched by addressing, forwarded, and frees the association.
    #[test]
    fn a_confirmation_is_forwarded_and_frees_the_association() {
        let mut s = server();
        request_in_progress(&mut s, Timestamp(0));
        let (_, _) = outputs(s.s_data_req(Timestamp(5), ai(ECU, TESTER), &[0x62], FINAL));
        let (out, ok) = outputs(s.t_data_conf(Timestamp(9), ai(ECU, TESTER), SResult::Ok));
        assert!(ok.is_ok());
        assert_eq!(
            out[0],
            Some(ServerOutput::Confirm {
                ai: ai(ECU, TESTER),
                result: SResult::Ok,
            })
        );
        let (_, ok) = outputs(s.s_data_req(Timestamp(10), ai(ECU, TESTER), &[0x62], FINAL));
        assert!(ok.is_ok());
    }

    /// ``UDSS_LLR_0085`` — a confirmed solicited positive response selecting a
    /// non-default session enters it, records the client and starts `tS3_Server`;
    /// ``UDSS_LLR_0088`` — a later confirmed final response restarts it.
    #[test]
    fn a_confirmed_session_selection_starts_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_000)));
        request_in_progress(&mut s, Timestamp(1_000));
        let (_, _) =
            outputs(s.s_data_req(Timestamp(1_005), ai(ECU, TESTER), &[0x62], FINAL));
        let (_, _) = outputs(s.t_data_conf(Timestamp(1_010), ai(ECU, TESTER), SResult::Ok));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_010)));
    }

    /// ``UDSS_LLR_0098`` — a confirmed response selecting the default session leaves the
    /// non-default one and disables `tS3_Server`.
    #[test]
    fn a_confirmed_default_selection_disables_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        request_in_progress(&mut s, Timestamp(1_000));
        let (_, _) = outputs(s.s_data_req(
            Timestamp(1_005),
            ai(ECU, TESTER),
            &[0x50, 0x01, 0, 50, 1, 244],
            ServerTx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: Some(SessionSelection::Default),
            },
        ));
        let (_, _) = outputs(s.t_data_conf(Timestamp(1_010), ai(ECU, TESTER), SResult::Ok));
        assert_eq!(s.next_deadline(), None);
    }

    /// ``UDSS_LLR_0093`` — a failed final response to the controlling client restarts
    /// `tS3_Server`; the session selection it carried does not take effect.
    #[test]
    fn a_failed_response_restarts_the_session_timer_and_selects_nothing() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        request_in_progress(&mut s, Timestamp(1_000));
        let (_, _) = outputs(s.s_data_req(
            Timestamp(1_005),
            ai(ECU, TESTER),
            &[0x50, 0x01],
            ServerTx::FinalResponse {
                solicitation: Solicitation::Solicited,
                session: Some(SessionSelection::Default),
            },
        ));
        let (_, _) = outputs(s.t_data_conf(
            Timestamp(1_010),
            ai(ECU, TESTER),
            SResult::Transport(TransportError(3)),
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_010)));
    }

    /// ``UDSS_LLR_0116``, ``UDSS_LLR_0110``, ``UDSS_LLR_0090`` — a confirmed
    /// response-pending opens the enhanced window, sets the anchor, and does not restart
    /// `tS3_Server`.
    #[test]
    fn a_confirmed_response_pending_opens_the_enhanced_window() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        request_in_progress(&mut s, Timestamp(1_000)); // stops tS3, starts tP2 at 1050
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(1_040),
            ai(ECU, TESTER),
            &[0x7F, 0x22, 0x78],
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), None); // tP2 stopped at the T_Data.req (0114)
        let (_, _) = outputs(s.t_data_conf(Timestamp(1_045), ai(ECU, TESTER), SResult::Ok));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_045))); // tP2* = 5000, tS3 stopped
    }

    /// ``UDSS_LLR_0118`` — a second response-pending while the first is unconfirmed is
    /// rejected; ``UDSS_LLR_0119`` — one inside the minimum spacing after the
    /// confirmation is rejected. Spacing is ⌈3 × 5000 / 10⌉ = 1500.
    #[test]
    fn response_pending_messages_are_spaced() {
        let mut s = server();
        request_in_progress(&mut s, Timestamp(0));
        let rp = [0x7F, 0x22, 0x78];
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(40),
            ai(ECU, TESTER),
            &rp,
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
        let (_, err) = outputs(s.s_data_req(
            Timestamp(41),
            ai(ECU, TESTER),
            &rp,
            ServerTx::ResponsePending,
        ));
        assert!(err.is_err_and(|r| r.contains(Cause::ResponsePendingUnconfirmed)));
        let (_, _) = outputs(s.t_data_conf(Timestamp(45), ai(ECU, TESTER), SResult::Ok));
        let (_, err) = outputs(s.s_data_req(
            Timestamp(1_544),
            ai(ECU, TESTER),
            &rp,
            ServerTx::ResponsePending,
        ));
        assert!(err.is_err_and(|r| r.contains(Cause::ResponsePendingTooSoon)));
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(1_545),
            ai(ECU, TESTER),
            &rp,
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
    }

    /// ``UDSS_LLR_0093`` (second limb) — a failed response-pending to the controlling
    /// client restarts `tS3_Server`; without it the session would be pinned open forever.
    #[test]
    fn a_failed_response_pending_restarts_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        request_in_progress(&mut s, Timestamp(1_000)); // tS3 stopped by the request
        let (_, _) = outputs(s.s_data_req(
            Timestamp(1_040),
            ai(ECU, TESTER),
            &[0x7F, 0x22, 0x78],
            ServerTx::ResponsePending,
        ));
        let (_, _) = outputs(s.t_data_conf(
            Timestamp(1_045),
            ai(ECU, TESTER),
            SResult::Transport(TransportError(9)),
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_045)));
    }
}

/// ``UDSS_LLR_0108``, ``UDSS_LLR_0109`` — a request from the same tester arriving before
/// the previous response's `T_Data.conf`, which ISO 14229-2:2021 10.3 lets the client
/// send, replaces the service in progress; that confirmation then answers nothing.
mod overlap {
    use super::*;
    use uds_session::Cause;

    const FINAL: ServerTx = ServerTx::FinalResponse {
        solicitation: Solicitation::Solicited,
        session: None,
    };
    const PENDING: [u8; 3] = [0x7F, 0x22, 0x78];

    fn request(s: &mut Server<2>, now: Timestamp) {
        let (_, ok) = outputs(s.t_data_ind(
            now,
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
    }

    fn submit(s: &mut Server<2>, now: Timestamp, class: ServerTx) -> Result<(), Rejection> {
        let data: &[u8] = match class {
            ServerTx::ResponsePending => &PENDING,
            ServerTx::FinalResponse { .. } => &[0x62, 0xF1, 0x90, 0x01],
        };
        outputs(s.s_data_req(now, ai(ECU, TESTER), data, class)).1
    }

    fn confirm(s: &mut Server<2>, now: Timestamp) {
        let (out, ok) = outputs(s.t_data_conf(now, ai(ECU, TESTER), SResult::Ok));
        assert!(ok.is_ok());
        assert!(matches!(out[0], Some(ServerOutput::Confirm { .. })));
    }

    fn overrun(loaded: ServerReload) -> ServerOutput<'static> {
        ServerOutput::ResponseOverrun {
            sa: Address(TESTER),
            ae: None,
            loaded,
        }
    }

    /// The first response's confirmation, arriving after the second request, leaves the
    /// second its `tP2_Server`: the deadline still names it and the overrun is reported.
    #[test]
    fn a_predecessors_confirmation_leaves_the_response_timer_running() {
        let mut s = server();
        request(&mut s, Timestamp(0));
        assert!(submit(&mut s, Timestamp(5), FINAL).is_ok());
        request(&mut s, Timestamp(10)); // tP2_Server due at 60
        confirm(&mut s, Timestamp(20));
        assert_eq!(s.next_deadline(), Some(Timestamp(60)));
        let (out, ok) = outputs(s.tick(Timestamp(60)));
        assert!(ok.is_ok());
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
        assert_eq!(out[1], None);
    }

    /// ``UDSS_LLR_0110``, ``UDSS_LLR_0116`` — the first request's response-pending
    /// confirmation neither opens the enhanced window for the second nor sets its
    /// anchor: the overrun names `tP2_Server_Max`, and a response-pending message sent
    /// on it is not refused as too soon (``UDSS_LLR_0119``).
    #[test]
    fn a_predecessors_pending_confirmation_opens_no_window() {
        let mut s = server();
        request(&mut s, Timestamp(0));
        assert!(submit(&mut s, Timestamp(40), ServerTx::ResponsePending).is_ok());
        request(&mut s, Timestamp(45)); // tP2_Server due at 95
        confirm(&mut s, Timestamp(46));
        assert_eq!(s.next_deadline(), Some(Timestamp(95)));
        let (out, _) = outputs(s.tick(Timestamp(95)));
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
        assert!(submit(&mut s, Timestamp(95), ServerTx::ResponsePending).is_ok());
    }

    /// ``UDSS_LLR_0059`` — the confirmation still frees its association: the second
    /// request's response, refused while the first was outstanding (``UDSS_LLR_0061``),
    /// is accepted after it.
    #[test]
    fn a_predecessors_confirmation_still_frees_its_association() {
        let mut s = server();
        request(&mut s, Timestamp(0));
        assert!(submit(&mut s, Timestamp(5), FINAL).is_ok());
        request(&mut s, Timestamp(10));
        let refused = submit(&mut s, Timestamp(15), FINAL);
        assert!(refused.is_err_and(|r| r.contains(Cause::AssociationOutstanding)));
        confirm(&mut s, Timestamp(20));
        assert!(submit(&mut s, Timestamp(25), FINAL).is_ok());
        assert_eq!(s.next_deadline(), None); // UDSS_LLR_0114 — the second's own response
    }

    /// ``UDSS_LLR_0088`` — the confirmation still acts on the session by its addressing:
    /// a final response to the controlling client restarts `tS3_Server`, while the
    /// second request keeps its `tP2_Server`.
    #[test]
    fn a_predecessors_confirmation_still_restarts_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        request(&mut s, Timestamp(1_000)); // stops tS3_Server (UDSS_LLR_0087)
        assert!(submit(&mut s, Timestamp(1_005), FINAL).is_ok());
        request(&mut s, Timestamp(1_010)); // tP2_Server due at 1060
        confirm(&mut s, Timestamp(1_020)); // tS3_Server restarted: due at 6020
        assert_eq!(s.next_deadline(), Some(Timestamp(1_060)));
        let (out, _) = outputs(s.tick(Timestamp(1_060)));
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
        assert_eq!(s.next_deadline(), Some(Timestamp(6_020)));
    }

    /// The inverse: once the first response's confirmation is in, the second request's
    /// own confirmations answer it — its response-pending confirmation opens the enhanced
    /// window and sets the anchor (``UDSS_LLR_0110``, ``UDSS_LLR_0116``), and its final
    /// response's ends it (``UDSS_LLR_0109``), so the anchor goes and a response-pending
    /// message inside the spacing is no longer refused (``UDSS_LLR_0119``).
    #[test]
    fn the_services_own_confirmation_still_answers_it() {
        let mut s = server();
        request(&mut s, Timestamp(0));
        assert!(submit(&mut s, Timestamp(5), FINAL).is_ok());
        request(&mut s, Timestamp(10));
        confirm(&mut s, Timestamp(20));
        assert!(submit(&mut s, Timestamp(30), ServerTx::ResponsePending).is_ok());
        confirm(&mut s, Timestamp(35));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_035)));
        let too_soon = submit(&mut s, Timestamp(36), ServerTx::ResponsePending);
        assert!(too_soon.is_err_and(|r| r.contains(Cause::ResponsePendingTooSoon)));
        assert!(submit(&mut s, Timestamp(40), FINAL).is_ok());
        assert_eq!(s.next_deadline(), None);
        confirm(&mut s, Timestamp(45));
        assert!(submit(&mut s, Timestamp(50), ServerTx::ResponsePending).is_ok());
    }
}

mod completion {
    use super::*;

    /// ``UDSS_LLR_0074``, ``UDSS_LLR_0115``, ``UDSS_LLR_0089`` — a completion report for
    /// a request answering the service in progress stops `tP2_Server` and, from the
    /// controlling client in a non-default session, restarts `tS3_Server`. No output.
    #[test]
    fn a_completion_report_restarts_the_session_timer() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(1_000),
            ai(TESTER, ECU),
            &[0x3E, 0x80],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        let (out, ok) = outputs(s.completion_report(
            Timestamp(1_010),
            ai(TESTER, ECU),
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
        assert_eq!(out[0], None);
        assert_eq!(s.next_deadline(), Some(Timestamp(6_010)));
    }

    /// ``UDSS_LLR_0086`` — a completed request selecting a non-default session, with no
    /// response, enters the session and starts the timer.
    #[test]
    fn a_suppressed_session_selection_enters_the_session() {
        let mut s = server();
        let (_, _) = outputs(s.t_data_ind(
            Timestamp(0),
            ai(TESTER, ECU),
            &[0x10, 0x83],
            SResult::Ok,
            ServerRx::Request {
                session: Some(SessionSelection::NonDefault),
            },
        ));
        let (_, _) = outputs(s.completion_report(
            Timestamp(5),
            ai(TESTER, ECU),
            ServerRx::Request {
                session: Some(SessionSelection::NonDefault),
            },
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_005)));
        let (out, _) = outputs(s.tick(Timestamp(5_005)));
        assert_eq!(
            out[0],
            Some(ServerOutput::SessionTimeout {
                client: peer(TESTER)
            })
        );
    }

    /// ``UDSS_LLR_0096`` — a keep-alive completion report changes nothing.
    #[test]
    fn a_keep_alive_completion_changes_nothing() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (_, _) = outputs(s.completion_report(
            Timestamp(100),
            ai(TESTER, ECU),
            ServerRx::KeepAlive,
        ));
        assert_eq!(s.next_deadline(), Some(Timestamp(5_000)));
    }

    /// ``UDSS_LLR_0038``, ``UDSS_LLR_0087`` — a start-of-message is not forwarded, and
    /// from the controlling client it stops `tS3_Server`.
    #[test]
    fn a_start_of_message_stops_the_session_timer_silently() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(0));
        let (out, ok) = outputs(s.t_data_som_ind(
            Timestamp(100),
            ai(TESTER, ECU),
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
        assert_eq!(out[0], None);
        assert_eq!(s.next_deadline(), None);
    }
}

mod lead {
    use super::*;
    use uds_session::ServerParameter;

    fn server_with(params: ServerParams) -> Server<2> {
        Server::new([Association::EMPTY, Association::EMPTY], params)
    }

    fn request(s: &mut Server<2>, now: Timestamp) {
        let (_, ok) = outputs(s.t_data_ind(
            now,
            ai(TESTER, ECU),
            &[0x22, 0xF1, 0x90],
            SResult::Ok,
            ServerRx::Request { session: None },
        ));
        assert!(ok.is_ok());
    }

    fn overrun(loaded: ServerReload) -> ServerOutput<'static> {
        ServerOutput::ResponseOverrun {
            sa: Address(TESTER),
            ae: None,
            loaded,
        }
    }

    const PENDING: [u8; 3] = [0x7F, 0x22, 0x78];

    /// ``UDSS_LLR_0117``, ``UDSS_LLR_0186`` — the overrun of `tP2_Server_Max` is
    /// indicated the lead before the window closes, and still names that window.
    #[test]
    fn the_default_overrun_is_indicated_the_lead_early() {
        let mut s = server_with(PARAMS_LEAD);
        request(&mut s, Timestamp(0));
        assert_eq!(s.next_deadline(), Some(Timestamp(40)));
        let (out, ok) = outputs(s.tick(Timestamp(39)));
        assert!(ok.is_ok());
        assert_eq!(out[0], None);
        let (out, ok) = outputs(s.tick(Timestamp(40)));
        assert!(ok.is_ok());
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
        assert_eq!(out[1], None);
        assert_eq!(s.next_deadline(), None);
        // The response-pending message sent on the indication goes out within the window.
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(40),
            ai(ECU, TESTER),
            &PENDING,
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
    }

    /// ``UDSS_LLR_0116``, ``UDSS_LLR_0186`` — the enhanced window a confirmed
    /// response-pending message opens is indicated the same lead early, at an instant
    /// ``UDSS_LLR_0119`` admits the next one.
    #[test]
    fn the_enhanced_overrun_is_indicated_the_lead_early() {
        let mut s = server_with(PARAMS_LEAD);
        request(&mut s, Timestamp(0));
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(30),
            ai(ECU, TESTER),
            &PENDING,
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
        let (_, ok) = outputs(s.t_data_conf(Timestamp(35), ai(ECU, TESTER), SResult::Ok));
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), Some(Timestamp(35 + 5_000 - 10)));
        let (out, _) = outputs(s.tick(Timestamp(5_024)));
        assert_eq!(out[0], None);
        let (out, _) = outputs(s.tick(Timestamp(5_025)));
        assert_eq!(out[0], Some(overrun(ServerReload::P2Star)));
        let (_, ok) = outputs(s.s_data_req(
            Timestamp(5_025),
            ai(ECU, TESTER),
            &PENDING,
            ServerTx::ResponsePending,
        ));
        assert!(ok.is_ok());
    }

    /// ``UDSS_LLR_0186`` — a lead not less than `tP2_Server_Max` saturates: the overrun
    /// is indicated on the first timestamp supplied after the request, never before it.
    #[test]
    fn a_lead_not_less_than_the_window_indicates_on_the_first_tick() {
        let params = ServerParams {
            response_pending_lead: 50,
            ..PARAMS
        };
        assert!(!params.is_well_formed());
        let mut s = server_with(params);
        request(&mut s, Timestamp(100));
        assert_eq!(s.next_deadline(), Some(Timestamp(100)));
        let (out, _) = outputs(s.tick(Timestamp(100)));
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
    }

    /// ``UDSS_LLR_0043``, ``UDSS_LLR_0186`` — the lead is taken when the window opens,
    /// so setting it again moves no window already open.
    #[test]
    fn a_lead_change_does_not_move_an_open_window() {
        let mut s = server_with(PARAMS_LEAD);
        request(&mut s, Timestamp(0));
        let (_, ok) = outputs(
            s.set_parameter(Timestamp(5), ServerParameter::ResponsePendingLead(30)),
        );
        assert!(ok.is_ok());
        assert_eq!(s.next_deadline(), Some(Timestamp(40)));
        let (out, _) = outputs(s.tick(Timestamp(40)));
        assert_eq!(out[0], Some(overrun(ServerReload::P2)));
        request(&mut s, Timestamp(60));
        assert_eq!(s.next_deadline(), Some(Timestamp(80)));
    }
}
