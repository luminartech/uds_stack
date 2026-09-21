//! Compile-time checks on the server surface, plus the storage wiring a caller must do.

use uds_session::{
    Address, Ai, Association, Mtype, SResult, Server, ServerParameter, ServerParams,
    ServerRx, ServerTx, Solicitation, TaType, Timestamp,
};

/// ``UDSS_LLR_0032`` — creation supplies the association storage of ``UDSS_LLR_0059`` and
/// the three parameters of ``UDSS_LLR_0042``. ``UDSS_LLR_0004`` is why the storage is the
/// caller's: the number of peers is a property of the deployment.
#[test]
fn a_server_is_created_from_caller_storage_and_three_parameters() {
    let _server = Server::new(
        [Association::EMPTY; 4],
        ServerParams {
            s3_server: 5_000,
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        },
    );
}

/// Type-checked, never run: every entry point the server has, with the types it takes.
/// ``UDSS_LLR_0030`` has six bullets. Its last three are discharged by the methods this
/// impl does *not* have — there is no `open_channel`, `withdraw_channel`, `reset_channel`
/// or `release_keep_alive` on a `Server`. Its third bullet, an indication identifying a
/// channel, is discharged by the absent channel parameter on `t_data_som_ind` and
/// `t_data_ind` below.
#[allow(dead_code, reason = "type-checked, never run")]
fn every_server_entry_point(server: &mut Server<4>, payload: &[u8]) {
    // ``UDSS_LLR_0080`` — a query the caller reads for itself, taking `&self`, not an
    // output in ``UDSS_LLR_0011``'s sense. It is `todo!()`, so it is type-checked here
    // rather than called from a running test.
    let _: Option<Timestamp> = server.next_deadline();

    let now = Timestamp(0);
    let ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0x10),
        ta: Address(0xF1),
        ta_type: TaType::Physical,
    };

    drop(server.set_parameter(now, ServerParameter::S3Server(5_000)));
    drop(server.s_data_req(
        now,
        ai,
        payload,
        ServerTx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: None,
        },
    ));
    drop(server.t_data_som_ind(now, ai, ServerRx::Request { session: None }));
    drop(server.t_data_ind(now, ai, payload, SResult::Ok, ServerRx::KeepAlive));
    drop(server.t_data_conf(now, ai, SResult::Ok));
    drop(server.completion_report(now, ai, ServerRx::Request { session: None }));
    drop(server.tick(now));
}
