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
    /// (Review focus 1.)
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
    #[ignore = "needs Task 6: t_data_som_ind"]
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
    /// nothing. (Review focus 2.)
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
