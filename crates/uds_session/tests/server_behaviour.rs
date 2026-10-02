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
    #[ignore = "enter_non_default needs s_data_req and t_data_conf (Task 5)"]
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
    #[ignore = "enter_non_default needs s_data_req and t_data_conf (Task 5)"]
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
    #[ignore = "enter_non_default needs s_data_req and t_data_conf (Task 5)"]
    fn the_session_timer_runs_across_the_wrap() {
        let mut s = server();
        enter_non_default(&mut s, Timestamp(u32::MAX - 999));
        assert_eq!(s.next_deadline(), Some(Timestamp(4_000)));
        let (out, _) = outputs(s.tick(Timestamp(3_999)));
        assert_eq!(out[0], None);
        let (out, _) = outputs(s.tick(Timestamp(4_000)));
        assert!(matches!(out[0], Some(ServerOutput::SessionTimeout { .. })));
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
        assert!(
            matches!(out[0], Some(ServerOutput::Indicate { result, .. }) if result == failed)
        );
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
    #[ignore = "needs Task 5: s_data_req/t_data_conf"]
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
    #[ignore = "needs Task 5: s_data_req/t_data_conf"]
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
    #[ignore = "needs Task 5: s_data_req/t_data_conf"]
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
    #[ignore = "needs Task 5: s_data_req/t_data_conf; Task 6: t_data_som_ind"]
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
