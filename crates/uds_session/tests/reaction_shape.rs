//! The reaction drain's shape and its two documented properties.

use uds_session::{Association, Server, ServerParams, ServerReaction, Timestamp};

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

/// ``UDSS_LLR_0011`` — outputs are drained by the caller, never pushed.
/// ``UDSS_LLR_0081`` — `finish` consumes the drain.
fn drained_then_finished(mut r: ServerReaction<'_, '_, 1>) -> bool {
    for _output in r.outputs() {}
    r.finish().is_ok()
}

/// A `tick` on a fresh server expires nothing and is accepted; draining it twice yields
/// nothing more, and the outcome is still reachable afterwards.
#[test]
fn a_fresh_tick_is_empty_and_accepted() {
    let mut server = Server::new([Association::EMPTY; 1], PARAMS);
    let mut r = server.tick(Timestamp(0));
    assert_eq!(r.outputs().count(), 0);
    assert_eq!(r.outputs().count(), 0);
    assert!(r.finish().is_ok());
    let r = server.tick(Timestamp(1));
    assert!(drained_then_finished(r));
}

/// Finishing without draining is accepted; the session is usable afterwards.
#[test]
fn finish_without_draining_is_accepted() {
    let mut server = Server::new([Association::EMPTY; 1], PARAMS);
    assert!(server.tick(Timestamp(0)).finish().is_ok());
    assert_eq!(server.next_deadline(), None);
}
