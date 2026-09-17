//! Compile-time checks on the client surface, and the storage wiring a caller must do.

use uds_session::{
    Address, Ai, ChannelId, Client, ClientRx, ClientTx, ExpectedResponses,
    FunctionalChannelParameter, FunctionalChannelParams, FunctionalKeepAlive,
    FunctionalSlot, KeepAliveMode, Mtype, PhysicalChannelParameter, PhysicalChannelParams,
    PhysicalSlot, Reloads, SResult, Solicitation, TaType, Timestamp,
};

/// ``UDSS_LLR_0121`` — a channel exists from the moment its storage is supplied.
/// ``UDSS_LLR_0139`` — the responder table's capacity is the number of entries that
/// storage holds, and a physical channel keeps no table at all.
///
/// Storage is supplied by value: the physical and functional arrays split by channel
/// kind, since only a functional channel carries a responder table.
#[test]
fn a_client_is_created_from_caller_storage() {
    let _client: Client<4, 1, 8> = Client::new(
        [PhysicalSlot::EMPTY; 4],
        [FunctionalSlot::EMPTY; 1],
        KeepAliveMode::Functional {
            storage: FunctionalKeepAlive::EMPTY,
            s3_client: 2_000,
        },
    );
}

/// ``UDSS_LLR_0151`` — in physical keep-alive the fact and timer live in each channel's
/// storage, so the mode carries nothing.
#[test]
fn physical_keep_alive_carries_no_client_wide_storage() {
    let _client: Client<2, 0, 8> =
        Client::new([PhysicalSlot::EMPTY; 2], [], KeepAliveMode::Physical);
}

/// Type-checked, never run: every entry point the client has.
/// ``UDSS_LLR_0031`` is discharged by the absence of `completion_report` and by
/// [`ClientTx`] and [`ClientRx`], neither of which can express the server's kinds.
///
/// `ch` is taken as a parameter, as `server_surface.rs` takes its stub values, rather than
/// constructed here: `ChannelId` has no public constructor of its own, only the one
/// [`Client::open_physical_channel`] and [`Client::open_functional_channel`] return, and
/// those methods are `todo!()`.
#[allow(dead_code, reason = "type-checked, never run")]
fn every_client_entry_point(client: &mut Client<4, 1, 8>, payload: &[u8], ch: ChannelId) {
    let now = Timestamp(0);
    let ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0xF1),
        ta: Address(0x10),
        ta_type: TaType::Physical,
    };
    let reloads = Reloads {
        default_reload: 50,
        enhanced_reload: 5_000,
    };
    let physical_params = PhysicalChannelParams {
        reloads,
        spacing: 60,
        s3_client: 2_000,
    };
    let functional_params = FunctionalChannelParams {
        reloads,
        spacing: 70,
    };

    let _: Option<Timestamp> = client.next_deadline();
    drop(client.open_physical_channel(now, ai, physical_params));
    drop(client.open_functional_channel(now, ai, functional_params));
    drop(client.withdraw_channel(now, ch));
    drop(client.set_physical_parameter(now, ch, PhysicalChannelParameter::Spacing(70)));
    drop(client.set_functional_parameter(now, ch, FunctionalChannelParameter::Spacing(70)));
    drop(client.reset_channel(now, ch));
    drop(client.release_keep_alive(now, ch));
    drop(client.s_data_req(
        now,
        ai,
        payload,
        ClientTx::Request {
            expected: ExpectedResponses::Unknown,
            repeat: false,
            session: None,
        },
    ));
    drop(client.t_data_som_ind(now, ch, ai, ClientRx::ResponsePending));
    drop(client.t_data_ind(
        now,
        ch,
        ai,
        payload,
        SResult::Ok,
        Some(ClientRx::FinalResponse {
            solicitation: Solicitation::Solicited,
            session: None,
        }),
    ));
    drop(client.t_data_conf(now, ai, SResult::Ok));
    drop(client.tick(now));
}
