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
#[expect(dead_code, reason = "used by the tests of Task 4")]
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
    #[ignore = "starting tP2_Server needs the t_data_ind body (Task 4)"]
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
