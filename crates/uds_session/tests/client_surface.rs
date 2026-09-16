//! Compile-time checks on the client surface, and the storage wiring a caller must do.

use uds_session::{
    Address, Ai, ChannelId, ChannelParameter, ChannelParams, ChannelSlot, Client, ClientRx,
    ClientTx, ExpectedResponses, FunctionalKeepAlive, KeepAliveMode, Mtype, ResponderSlot,
    SResult, Solicitation, TaType, Timestamp,
};

/// ``UDSS_LLR_0121`` — a channel exists from the moment its storage is supplied.
/// ``UDSS_LLR_0139`` — the responder table's capacity is the number of entries that
/// storage holds, and a physical channel keeps no table at all.
///
/// `Client<'s, 'r>` borrows the channel array for `'s` and each responder array for `'r`.
/// Both are ordinary borrows: no particular declaration order is required, only that each
/// responder array outlives the client that holds it.
#[test]
fn a_client_is_created_from_caller_storage() {
    let mut responders = [ResponderSlot::EMPTY; 8];
    let mut keep_alive = FunctionalKeepAlive::EMPTY;
    let mut channels = [ChannelSlot::EMPTY; 4];

    let _client = Client::new(
        &mut channels,
        KeepAliveMode::Functional {
            storage: &mut keep_alive,
            s3_client: 2_000,
        },
    );
    // `responders` is handed to `open_channel`, which is `todo!()` at this stage.
    let _ = &mut responders;
}

/// ``UDSS_LLR_0151`` — in physical keep-alive the fact and timer live in each channel's
/// storage, so the mode carries nothing.
#[test]
fn physical_keep_alive_carries_no_client_wide_storage() {
    let mut channels = [ChannelSlot::EMPTY; 2];
    let _client = Client::new(&mut channels, KeepAliveMode::Physical);
}

/// Type-checked, never run: every entry point the client has.
/// ``UDSS_LLR_0031`` is discharged by the absence of `completion_report` and by
/// [`ClientTx`] and [`ClientRx`], neither of which can express the server's kinds.
///
/// `ch` is taken as a parameter, as `server_surface.rs` takes its stub values, rather than
/// constructed here: `ChannelId` has no public constructor of its own, only the one
/// [`Client::open_channel`] returns, and that method is `todo!()`.
#[allow(dead_code, reason = "type-checked, never run")]
fn every_client_entry_point<'r>(
    client: &mut Client<'_, 'r>,
    responders: &'r mut [ResponderSlot],
    payload: &[u8],
    ch: ChannelId,
) {
    let now = Timestamp(0);
    let ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0xF1),
        ta: Address(0x10),
        ta_type: TaType::Physical,
    };
    let params = ChannelParams {
        default_reload: 50,
        enhanced_reload: 5_000,
        spacing: 60,
        s3_client: None,
    };

    let _: Option<Timestamp> = client.next_deadline();
    drop(client.open_channel(now, ai, params, responders));
    drop(client.withdraw_channel(now, ch));
    drop(client.set_parameter(now, ch, ChannelParameter::Spacing(70)));
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
