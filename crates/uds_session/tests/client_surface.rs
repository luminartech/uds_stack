//! Compile-time checks on the client surface, and the storage wiring a caller must do.

use uds_session::{
    Address, Ai, ChannelAddressing, ChannelId, ChannelParameter, ChannelParams, Client,
    ClientRx, ClientTx, ExpectedResponses, FunctionalChannelId, FunctionalKeepAlive,
    FunctionalSlot, Mtype, PhysicalChannelId, PhysicalKeepAlive, PhysicalSlot, Reloads,
    SResult, Solicitation, TaType, Timestamp,
};

/// ``UDSS_LLR_0121`` — a channel exists from the moment the caller opens it.
/// ``UDSS_LLR_0139`` — the responder table's capacity is the number of entries that
/// storage holds, and a physical channel keeps no table at all.
///
/// Storage is supplied by value: the physical and functional arrays split by channel
/// kind, since only a functional channel carries a responder table.
#[test]
fn a_client_is_created_from_caller_storage() {
    let _client: Client<FunctionalKeepAlive, 4, 1, 8> = Client::new(
        [PhysicalSlot::EMPTY; 4],
        [FunctionalSlot::EMPTY; 1],
        FunctionalKeepAlive::new(2_000),
    );
}

/// ``UDSS_LLR_0151`` — in physical keep-alive the fact and timer live in each channel's
/// storage, so the mode carries nothing.
///
/// `R` is left to its default: with `FUNC` of `0` there is no responder table to size, so
/// the caller names no capacity for one.
#[test]
fn physical_keep_alive_carries_no_client_wide_storage() {
    let _client: Client<PhysicalKeepAlive, 2, 0> =
        Client::new([PhysicalSlot::EMPTY; 2], [], PhysicalKeepAlive);
}

/// Type-checked, never run: every entry point the client has.
/// ``UDSS_LLR_0031`` is discharged by the absence of `completion_report` and by
/// [`ClientTx`] and [`ClientRx`], neither of which can express the server's kinds.
///
/// `ch_phys` and `ch_func` are taken as parameters, as `server_surface.rs` takes its stub
/// values, rather than constructed here: neither id has a public constructor of its own,
/// only the one [`Client::open_physical_channel`] and [`Client::open_functional_channel`]
/// each return, and a function that is type-checked and never run has no client to open
/// one on. Each flows to its own kind's setter
/// directly, and to the kind-agnostic methods directly as well: those take
/// `impl Into<`[`ChannelId`]`>`, so both kinds pass without a widening step at the call.
/// [`Client::reset_channel`] is passed a [`ChannelId`] itself, since the widened parameter
/// has to keep accepting the type it replaced.
#[allow(dead_code, reason = "type-checked, never run")]
fn every_client_entry_point(
    client: &mut Client<FunctionalKeepAlive, 4, 1, 8>,
    payload: &[u8],
    ch_phys: PhysicalChannelId,
    ch_func: FunctionalChannelId,
) {
    let now = Timestamp(0);
    let ai = Ai {
        mtype: Mtype::Diag,
        sa: Address(0xF1),
        ta: Address(0x10),
        ta_type: TaType::Physical,
    };
    let addressing = ChannelAddressing {
        mtype: Mtype::Diag,
        sa: Address(0xF1),
        ta: Address(0x10),
    };
    let reloads = Reloads {
        default_reload: 50,
        enhanced_reload: 5_000,
    };
    let physical_params = ChannelParams {
        reloads,
        spacing: 60,
    };
    let functional_params = ChannelParams {
        reloads,
        spacing: 70,
    };

    let _: Option<Timestamp> = client.next_deadline();
    drop(client.open_physical_channel(now, addressing, physical_params));
    drop(client.open_functional_channel(now, addressing, functional_params));
    drop(client.withdraw_channel(now, ch_phys));
    drop(client.set_physical_parameter(now, ch_phys, ChannelParameter::Spacing(70)));
    drop(client.set_functional_parameter(now, ch_func, ChannelParameter::Spacing(70)));
    drop(client.set_keep_alive_reload(now, 2_000));
    drop(client.reset_channel(now, ChannelId::Functional(ch_func)));
    drop(client.release_keep_alive(now, ch_phys));
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
    drop(client.t_data_som_ind(now, ch_func, ai, ClientRx::ResponsePending));
    drop(client.t_data_ind(
        now,
        ch_phys,
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

/// ``UDSS_LLR_0152`` — the physical-keep-alive surface, which differs from the functional
/// one in exactly the two places that requirement names: opening a physical channel
/// carries this channel's reload, and there is no client-wide reload to set.
#[allow(dead_code, reason = "type-checked, never run")]
fn every_physical_keep_alive_entry_point(
    client: &mut Client<PhysicalKeepAlive, 2, 0>,
    ch_phys: PhysicalChannelId,
) {
    let now = Timestamp(0);
    let addressing = ChannelAddressing {
        mtype: Mtype::Diag,
        sa: Address(0xF1),
        ta: Address(0x10),
    };
    let params = ChannelParams {
        reloads: Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        },
        spacing: 60,
    };

    drop(client.open_physical_channel(now, addressing, params, 2_000));
    drop(client.set_physical_s3_client(now, ch_phys, 3_000));
}
