//! Behavioural tests of the client role, one per requirement a body satisfies.

use core::num::NonZeroU16;

use uds_session::{
    Address, Ai, Cause, ChannelAddressing, ChannelId, ChannelParameter, ChannelParams,
    ChannelReload, Client, ClientOutput, ClientReaction, ClientRx, ClientTx, Content,
    ExpectedResponses, FunctionalChannelId, FunctionalKeepAlive, FunctionalSlot,
    KeepAliveMode, Mtype, PhysicalChannelId, PhysicalSlot, Rejection, Reloads, SResult,
    Solicitation, TaType, Timestamp, TransportError,
};

const TESTER: u16 = 0x0E80;
const ECU: u16 = 0x0010;
const ECU_2: u16 = 0x0011;
const FUNCTIONAL: u16 = 0x0FFF;
const RELOADS: Reloads = Reloads {
    default_reload: 50,
    enhanced_reload: 5_000,
};
const PHYS_PARAMS: ChannelParams = ChannelParams {
    reloads: RELOADS,
    spacing: 60,
};
/// A short `tP3_Client_Func`, so that the spacing every functional confirmation starts
/// has expired by the next input of the response tests; the spacing tests use
/// [`SPACED_FUNC`].
const FUNC_PARAMS: ChannelParams = ChannelParams {
    reloads: RELOADS,
    spacing: 5,
};
const SPACED_FUNC: ChannelParams = ChannelParams {
    reloads: RELOADS,
    spacing: 70,
};
const S3_CLIENT: u32 = 2_000;
const DATA: [u8; 3] = [0x22, 0xF1, 0x90];

type Tester = Client<FunctionalKeepAlive, 2, 1, 2>;

fn tester() -> Tester {
    Client::new(
        [PhysicalSlot::EMPTY; 2],
        [FunctionalSlot::EMPTY; 1],
        FunctionalKeepAlive::new(S3_CLIENT),
    )
}

/// A client with one physical slot and no functional one.
fn single() -> Client<FunctionalKeepAlive, 1, 0> {
    Client::new(
        [PhysicalSlot::EMPTY],
        [],
        FunctionalKeepAlive::new(S3_CLIENT),
    )
}

fn to(ta: u16) -> ChannelAddressing {
    ChannelAddressing {
        mtype: Mtype::Diag,
        sa: Address(TESTER),
        ta: Address(ta),
    }
}

fn req(ta: u16, ta_type: TaType) -> Ai {
    to(ta).with_ta_type(ta_type)
}

fn phys(ta: u16) -> Ai {
    req(ta, TaType::Physical)
}

/// A response from `sa` to the tester.
fn rsp(sa: u16) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: Address(sa),
        ta: Address(TESTER),
        ta_type: TaType::Physical,
    }
}

fn func() -> Ai {
    req(FUNCTIONAL, TaType::Functional)
}

const fn request(expected: ExpectedResponses) -> ClientTx {
    ClientTx::Request {
        expected,
        repeat: false,
        session: None,
    }
}

const UNKNOWN: ClientTx = request(ExpectedResponses::Unknown);
const NO_RESPONSE: ClientTx = request(ExpectedResponses::None);
const ONE: ExpectedResponses = ExpectedResponses::Exactly(NonZeroU16::MIN);
const SOLICITED: ClientRx = ClientRx::FinalResponse {
    solicitation: Solicitation::Solicited,
    session: None,
};
const UNSOLICITED: ClientRx = ClientRx::FinalResponse {
    solicitation: Solicitation::Unsolicited,
    session: None,
};
const PENDING: ClientRx = ClientRx::ResponsePending;
const FAILED: SResult = SResult::Transport(TransportError(1));

const NOTHING: [Option<ClientOutput<'static>>; 6] = [None; 6];

/// Drain a reaction into a fixed array (no alloc), then finish it.
fn outputs<'d, K: KeepAliveMode, const P: usize, const F: usize, const R: usize, T>(
    mut r: ClientReaction<'_, 'd, K, P, F, R, T>,
) -> ([Option<ClientOutput<'d>>; 6], Result<T, Rejection>) {
    let mut out = [None; 6];
    for (slot, o) in out.iter_mut().zip(r.outputs()) {
        *slot = Some(o);
    }
    (out, r.finish())
}

/// The drained outputs, exactly `first` and nothing after it.
const fn only(first: ClientOutput<'_>) -> [Option<ClientOutput<'_>>; 6] {
    [Some(first), None, None, None, None, None]
}

fn rejected<T>(outcome: Result<T, Rejection>, cause: Cause) -> bool {
    outcome.is_err_and(|r| r.contains(cause))
}

#[allow(
    clippy::panic,
    reason = "a test harness: a test cannot go on without the handle"
)]
fn open_phys<const P: usize, const F: usize, const R: usize>(
    c: &mut Client<FunctionalKeepAlive, P, F, R>,
    now: Timestamp,
    ta: u16,
) -> PhysicalChannelId {
    let (_, id) = outputs(c.open_physical_channel(now, to(ta), PHYS_PARAMS));
    id.unwrap_or_else(|r| panic!("open failed: {r}"))
}

#[allow(
    clippy::panic,
    reason = "a test harness: a test cannot go on without the handle"
)]
fn open_func_with<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
    params: ChannelParams,
) -> FunctionalChannelId {
    let (_, id) = outputs(c.open_functional_channel(now, to(FUNCTIONAL), params));
    id.unwrap_or_else(|r| panic!("open failed: {r}"))
}

fn open_func<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
) -> FunctionalChannelId {
    open_func_with(c, now, FUNC_PARAMS)
}

/// Send and confirm one request at `now`; any response window opens from `now`.
fn exchange<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
    ai: Ai,
    class: ClientTx,
) {
    let (_, sent) = outputs(c.s_data_req(now, ai, &DATA, class));
    assert_eq!(sent, Ok(()));
    let (_, confirmed) = outputs(c.t_data_conf(now, ai, SResult::Ok));
    assert_eq!(confirmed, Ok(()));
}

/// A completed message from `sa` on `channel`, accepted; its outputs.
fn ind<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
    channel: impl Into<ChannelId>,
    sa: u16,
    result: SResult,
    class: Option<ClientRx>,
) -> [Option<ClientOutput<'static>>; 6] {
    let (out, accepted) =
        outputs(c.t_data_ind(now, channel, rsp(sa), &DATA, result, class));
    assert_eq!(accepted, Ok(()));
    out
}

/// A started message from `sa` on `channel`, accepted; its outputs.
fn som<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
    channel: impl Into<ChannelId>,
    sa: u16,
    class: ClientRx,
) -> [Option<ClientOutput<'static>>; 6] {
    let (out, accepted) = outputs(c.t_data_som_ind(now, channel, rsp(sa), class));
    assert_eq!(accepted, Ok(()));
    out
}

fn indicate(
    channel: impl Into<ChannelId>,
    sa: u16,
    result: SResult,
) -> ClientOutput<'static> {
    ClientOutput::Indicate {
        channel: channel.into(),
        ai: rsp(sa),
        data: &DATA,
        result,
    }
}

const fn timeout(ai: Ai, loaded: ChannelReload) -> ClientOutput<'static> {
    ClientOutput::ResponseTimeout { ai, loaded }
}

mod channels {
    use super::*;

    /// ``UDSS_LLR_0121`` — opening returns the handle the channel is then named by.
    #[test]
    fn opening_returns_a_handle_the_channel_is_named_by() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        let (_, set) = outputs(c.set_physical_parameter(
            Timestamp(0),
            id,
            ChannelParameter::Spacing(10),
        ));
        assert_eq!(set, Ok(()));
    }

    /// ``UDSS_LLR_0121`` — a handle is never reissued, so a withdrawn channel's handle
    /// does not name the channel later opened in its slot.
    #[test]
    fn a_withdrawn_handle_does_not_name_a_channel_reopened_in_its_slot() {
        let mut c = single();
        let a = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), a)).1, Ok(()));
        let b = open_phys(&mut c, Timestamp(0), ECU);
        assert_ne!(a, b);
        assert!(rejected(
            outputs(c.withdraw_channel(Timestamp(0), a)).1,
            Cause::NoSuchChannel
        ));
        let (_, set) = outputs(c.set_physical_parameter(
            Timestamp(0),
            b,
            ChannelParameter::Spacing(10),
        ));
        assert_eq!(set, Ok(()));
    }

    /// ``UDSS_LLR_0122`` — an addressing equal to an existing channel's is refused; the
    /// same addresses with the other `TAtype` are a different addressing.
    #[test]
    fn duplicate_channel_addressing_is_rejected() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        let (out, again) =
            outputs(c.open_physical_channel(Timestamp(0), to(ECU), PHYS_PARAMS));
        assert_eq!(out, NOTHING);
        assert!(rejected(again, Cause::DuplicateChannelAddressing));
        let (_, functional) =
            outputs(c.open_functional_channel(Timestamp(0), to(ECU), FUNC_PARAMS));
        assert!(functional.is_ok());
    }

    /// ``UDSS_LLR_0185`` (first limb) — an open with every slot of its kind in use is
    /// refused, and withdrawal frees the slot.
    #[test]
    fn an_open_with_no_free_slot_is_rejected_and_withdrawal_frees_one() {
        let mut c = single();
        let first = open_phys(&mut c, Timestamp(0), ECU);
        let (_, full) =
            outputs(c.open_physical_channel(Timestamp(0), to(ECU_2), PHYS_PARAMS));
        assert!(rejected(full, Cause::NoChannelSlotFree));
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), first)).1, Ok(()));
        let (_, freed) =
            outputs(c.open_physical_channel(Timestamp(0), to(ECU_2), PHYS_PARAMS));
        assert!(freed.is_ok());
    }

    /// ``UDSS_LLR_0016`` with ``UDSS_LLR_0122`` and ``UDSS_LLR_0185`` — one report
    /// states every cause that held.
    #[test]
    fn one_report_states_a_duplicate_and_a_full_store() {
        let mut c = single();
        open_phys(&mut c, Timestamp(0), ECU);
        let (_, again) =
            outputs(c.open_physical_channel(Timestamp(0), to(ECU), PHYS_PARAMS));
        assert!(rejected(again, Cause::DuplicateChannelAddressing));
        assert!(rejected(again, Cause::NoChannelSlotFree));
    }

    /// ``UDSS_LLR_0124`` — a withdrawal naming no channel the client has is refused.
    #[test]
    fn a_withdrawal_naming_no_channel_is_rejected() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), id)).1, Ok(()));
        let (out, again) = outputs(c.withdraw_channel(Timestamp(0), id));
        assert_eq!(out, NOTHING);
        assert!(rejected(again, Cause::NoSuchChannel));
    }

    /// ``UDSS_LLR_0134`` — a parameter setting naming no channel the client has is
    /// refused, for either kind.
    #[test]
    fn a_parameter_setting_naming_no_channel_is_rejected() {
        let mut c = tester();
        let p = open_phys(&mut c, Timestamp(0), ECU);
        let f = open_func(&mut c, Timestamp(0));
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), p)).1, Ok(()));
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), f)).1, Ok(()));
        let spacing = ChannelParameter::Spacing(10);
        assert!(rejected(
            outputs(c.set_physical_parameter(Timestamp(0), p, spacing)).1,
            Cause::NoSuchChannel
        ));
        assert!(rejected(
            outputs(c.set_functional_parameter(Timestamp(0), f, spacing)).1,
            Cause::NoSuchChannel
        ));
    }

    /// ``UDSS_LLR_0127``, ``UDSS_LLR_0153``, ``UDSS_LLR_0167`` — no timer runs in a
    /// fresh client or on a freshly opened channel.
    #[test]
    fn a_fresh_client_and_a_fresh_channel_run_no_timer() {
        let mut c = tester();
        assert_eq!(c.next_deadline(), None);
        open_phys(&mut c, Timestamp(0), ECU);
        open_func(&mut c, Timestamp(0));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0010``, ``UDSS_LLR_0121`` — opening a channel produces no output.
    #[test]
    fn an_open_produces_no_output() {
        let mut c = tester();
        let (out, id) =
            outputs(c.open_physical_channel(Timestamp(0), to(ECU), PHYS_PARAMS));
        assert_eq!(out, NOTHING);
        assert!(id.is_ok());
    }
}

mod request {
    use super::*;

    /// ``UDSS_LLR_0033``, ``UDSS_LLR_0024`` — a request is handed to the transport on
    /// the channel its addressing names.
    #[test]
    fn a_request_is_transmitted_on_its_channel() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        let (out, sent) = outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, UNKNOWN));
        assert_eq!(
            out,
            only(ClientOutput::Transmit {
                channel: ChannelId::from(id),
                ai: phys(ECU),
                data: &DATA,
            })
        );
        assert_eq!(sent, Ok(()));
    }

    /// ``UDSS_LLR_0123``, ``UDSS_LLR_0015`` — a request naming no channel is refused and
    /// nothing is sent.
    #[test]
    fn a_request_naming_no_channel_is_rejected() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        let (out, sent) = outputs(c.s_data_req(Timestamp(0), phys(ECU_2), &DATA, UNKNOWN));
        assert_eq!(out, NOTHING);
        assert!(rejected(sent, Cause::NoSuchChannel));
    }

    /// ``UDSS_LLR_0061``, ``UDSS_LLR_0060`` — a second request before the first is
    /// confirmed duplicates its association and is refused.
    #[test]
    fn a_request_duplicating_an_outstanding_association_is_rejected() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(
            outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, UNKNOWN)).1,
            Ok(())
        );
        let (out, again) = outputs(c.s_data_req(Timestamp(1), phys(ECU), &DATA, UNKNOWN));
        assert_eq!(out, NOTHING);
        assert!(rejected(again, Cause::AssociationOutstanding));
    }

    /// ``UDSS_LLR_0025``, ``UDSS_LLR_0037``, ``UDSS_LLR_0039``, ``UDSS_LLR_0059`` — a
    /// confirmation is forwarded and frees the association it matches.
    #[test]
    fn a_confirmation_is_forwarded_and_frees_the_association() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(
            outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, NO_RESPONSE)).1,
            Ok(())
        );
        let (out, confirmed) = outputs(c.t_data_conf(Timestamp(1), phys(ECU), SResult::Ok));
        assert_eq!(
            out,
            only(ClientOutput::Confirm {
                ai: phys(ECU),
                result: SResult::Ok,
            })
        );
        assert_eq!(confirmed, Ok(()));
        let (_, again) = outputs(c.s_data_req(Timestamp(1_000), phys(ECU), &DATA, UNKNOWN));
        assert_eq!(again, Ok(()));
    }

    /// ``UDSS_LLR_0059`` — the match is on the whole addressing, message type included.
    #[test]
    fn a_confirmation_matches_on_the_message_type() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(
            outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, UNKNOWN)).1,
            Ok(())
        );
        let secure = Ai {
            mtype: Mtype::SecureDiag,
            ..phys(ECU)
        };
        let (out, confirmed) = outputs(c.t_data_conf(Timestamp(1), secure, SResult::Ok));
        assert_eq!(out, NOTHING);
        assert!(rejected(confirmed, Cause::NoMatchingAssociation));
    }

    /// ``UDSS_LLR_0063``, ``UDSS_LLR_0064`` — a confirmation matching nothing is refused.
    #[test]
    fn a_confirmation_matching_nothing_is_rejected() {
        let mut c = tester();
        let (out, confirmed) = outputs(c.t_data_conf(Timestamp(0), phys(ECU), SResult::Ok));
        assert_eq!(out, NOTHING);
        assert!(rejected(confirmed, Cause::NoMatchingAssociation));
    }

    /// ``UDSS_LLR_0125``, ``UDSS_LLR_0063`` — withdrawal discards the outstanding
    /// association without output, so its late confirmation matches nothing.
    #[test]
    fn withdrawal_discards_an_outstanding_association() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(
            outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, UNKNOWN)).1,
            Ok(())
        );
        let (out, withdrawn) = outputs(c.withdraw_channel(Timestamp(1), id));
        assert_eq!(out, NOTHING);
        assert_eq!(withdrawn, Ok(()));
        let (_, confirmed) = outputs(c.t_data_conf(Timestamp(2), phys(ECU), SResult::Ok));
        assert!(rejected(confirmed, Cause::NoMatchingAssociation));
    }

    /// ``UDSS_LLR_0135``, ``UDSS_LLR_0128`` — a confirmed request expecting a response
    /// opens the window with the default reload.
    #[test]
    fn the_window_opens_on_a_confirmed_request_expecting_a_response() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
    }

    /// ``UDSS_LLR_0135`` — a request expecting no response opens no window.
    #[test]
    fn a_request_expecting_none_opens_no_window() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        let (_, sent) = outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, NO_RESPONSE));
        assert_eq!(sent, Ok(()));
        let (_, confirmed) = outputs(c.t_data_conf(Timestamp(0), phys(ECU), SResult::Ok));
        assert_eq!(confirmed, Ok(()));
        let (out, _) = outputs(c.tick(Timestamp(1_000)));
        assert_eq!(out, NOTHING);
    }

    /// ``UDSS_LLR_0135`` — a transmission that failed opens no window.
    #[test]
    fn a_failed_transmission_opens_no_window() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        let (_, sent) = outputs(c.s_data_req(Timestamp(0), phys(ECU), &DATA, UNKNOWN));
        assert_eq!(sent, Ok(()));
        let failed = FAILED;
        let (out, confirmed) = outputs(c.t_data_conf(Timestamp(0), phys(ECU), failed));
        assert_eq!(
            out,
            only(ClientOutput::Confirm {
                ai: phys(ECU),
                result: failed,
            })
        );
        assert_eq!(confirmed, Ok(()));
        let (out, _) = outputs(c.tick(Timestamp(1_000)));
        assert_eq!(out, NOTHING);
    }
}

mod expiry {
    use super::*;

    /// ``UDSS_LLR_0148``, ``UDSS_LLR_0077`` — `tP_Client` expires only once its loaded
    /// value is exceeded.
    #[test]
    fn the_window_expires_only_once_exceeded() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        assert_eq!(outputs(c.tick(Timestamp(50))).0, NOTHING);
        assert_eq!(
            outputs(c.tick(Timestamp(51))).0,
            only(timeout(phys(ECU), ChannelReload::Default))
        );
    }

    /// ``UDSS_LLR_0080`` — the deadline of an "exceeds" timer is one past its window, so
    /// a caller waking exactly then finds the expiry rather than nothing.
    #[test]
    fn the_response_deadline_is_one_past_the_window() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
        assert_eq!(
            outputs(c.tick(Timestamp(51))).0,
            only(timeout(phys(ECU), ChannelReload::Default))
        );
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0148``, ``UDSS_LLR_0078`` — an expired timer is stopped, so the
    /// timeout is indicated once.
    #[test]
    fn a_timeout_is_indicated_once() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        assert_ne!(outputs(c.tick(Timestamp(51))).0, NOTHING);
        assert_eq!(outputs(c.tick(Timestamp(52))).0, NOTHING);
    }

    /// ``UDSS_LLR_0019`` — a window opened just before the timestamp wrap expires just
    /// after it.
    #[test]
    fn the_response_window_runs_across_the_wrap() {
        let mut c = tester();
        let start = Timestamp(u32::MAX - 9);
        open_phys(&mut c, start, ECU);
        exchange(&mut c, start, phys(ECU), UNKNOWN);
        assert_eq!(c.next_deadline(), Some(Timestamp(41)));
        assert_eq!(outputs(c.tick(Timestamp(40))).0, NOTHING);
        assert_eq!(
            outputs(c.tick(Timestamp(41))).0,
            only(timeout(phys(ECU), ChannelReload::Default))
        );
    }

    /// ``UDSS_LLR_0081``, ``UDSS_LLR_0015`` — an expiry the input's timestamp causes is
    /// indicated before the input is acted on, even when the input is refused.
    #[test]
    fn an_expiry_precedes_the_input_that_carried_its_timestamp() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        let (out, sent) = outputs(c.s_data_req(Timestamp(51), phys(ECU_2), &DATA, UNKNOWN));
        assert_eq!(out, only(timeout(phys(ECU), ChannelReload::Default)));
        assert!(rejected(sent, Cause::NoSuchChannel));
    }

    /// ``UDSS_LLR_0010``, ``UDSS_LLR_0081`` — a withdrawal at an expiry's timestamp still
    /// indicates the expiry, though it discards the channel.
    #[test]
    fn an_expiry_at_a_withdrawal_is_still_indicated() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        let (out, withdrawn) = outputs(c.withdraw_channel(Timestamp(51), id));
        assert_eq!(out, only(timeout(phys(ECU), ChannelReload::Default)));
        assert_eq!(withdrawn, Ok(()));
    }

    /// ``UDSS_LLR_0120``, ``UDSS_LLR_0126`` — each channel keeps its own window.
    #[test]
    fn two_channels_keep_their_own_windows() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        open_phys(&mut c, Timestamp(0), ECU_2);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        exchange(&mut c, Timestamp(20), phys(ECU_2), UNKNOWN);
        assert_eq!(
            outputs(c.tick(Timestamp(51))).0,
            only(timeout(phys(ECU), ChannelReload::Default))
        );
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
    }

    /// ``UDSS_LLR_0081`` — expiries at one timestamp are indicated in slot order.
    #[test]
    fn simultaneous_expiries_drain_in_slot_order() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        open_phys(&mut c, Timestamp(0), ECU_2);
        open_func(&mut c, Timestamp(0));
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        exchange(&mut c, Timestamp(0), phys(ECU_2), UNKNOWN);
        exchange(&mut c, Timestamp(0), func(), UNKNOWN);
        assert_eq!(
            outputs(c.tick(Timestamp(51))).0,
            [
                Some(timeout(phys(ECU), ChannelReload::Default)),
                Some(timeout(phys(ECU_2), ChannelReload::Default)),
                Some(timeout(func(), ChannelReload::Default)),
                None,
                None,
                None,
            ]
        );
    }

    /// ``UDSS_LLR_0076``, ``UDSS_LLR_0043`` — a parameter change applies from the next
    /// start; it does not move a window already open.
    #[test]
    fn a_parameter_change_does_not_move_an_open_window() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        let (_, set) = outputs(c.set_physical_parameter(
            Timestamp(10),
            id,
            ChannelParameter::DefaultReload(500),
        ));
        assert_eq!(set, Ok(()));
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
    }

    /// ``UDSS_LLR_0079`` — a window of zero expires on the first later timestamp.
    #[test]
    fn a_zero_reload_expires_on_the_first_later_timestamp() {
        let mut c = tester();
        let zero = ChannelParams {
            reloads: Reloads {
                default_reload: 0,
                enhanced_reload: 0,
            },
            spacing: 0,
        };
        let (_, id) = outputs(c.open_physical_channel(Timestamp(0), to(ECU), zero));
        assert!(id.is_ok());
        exchange(&mut c, Timestamp(5), phys(ECU), UNKNOWN);
        assert_eq!(outputs(c.tick(Timestamp(5))).0, NOTHING);
        assert_eq!(
            outputs(c.tick(Timestamp(6))).0,
            only(timeout(phys(ECU), ChannelReload::Default))
        );
    }
}

mod physical_window {
    use super::*;

    fn exchanged(now: Timestamp) -> (Tester, PhysicalChannelId) {
        let mut c = tester();
        let id = open_phys(&mut c, now, ECU);
        exchange(&mut c, now, phys(ECU), UNKNOWN);
        (c, id)
    }

    /// ``UDSS_LLR_0038``, ``UDSS_LLR_0023`` — a start-of-message is never forwarded.
    #[test]
    fn a_start_of_message_is_not_forwarded() {
        let (mut c, id) = exchanged(Timestamp(0));
        assert_eq!(som(&mut c, Timestamp(10), id, ECU, SOLICITED), NOTHING);
    }

    /// ``UDSS_LLR_0136``, ``UDSS_LLR_0128`` — a solicited final response's
    /// start-of-message ends the request and stops `tP_Client`.
    #[test]
    fn a_solicited_final_start_of_message_closes_the_window() {
        let (mut c, id) = exchanged(Timestamp(0));
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        assert_eq!(c.next_deadline(), None);
        assert_eq!(outputs(c.tick(Timestamp(100))).0, NOTHING);
    }

    /// ``UDSS_LLR_0136``, ``UDSS_LLR_0045``, ``UDSS_LLR_0036`` — so does a solicited
    /// final response arriving whole, which is indicated.
    #[test]
    fn a_solicited_final_single_frame_closes_the_window() {
        let (mut c, id) = exchanged(Timestamp(0));
        let out = ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(out, only(indicate(id, ECU, SResult::Ok)));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0136`` — an unsolicited response leaves the window as it was.
    #[test]
    fn an_unsolicited_response_leaves_the_window_open() {
        let (mut c, id) = exchanged(Timestamp(0));
        ind(
            &mut c,
            Timestamp(10),
            id,
            ECU,
            SResult::Ok,
            Some(UNSOLICITED),
        );
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
    }

    /// ``UDSS_LLR_0136``, ``UDSS_LLR_0128`` — a response-pending start-of-message stops
    /// the timer but the request stays in progress, so its completion restarts it.
    #[test]
    fn a_response_pending_start_stops_the_timer_but_not_the_request() {
        let (mut c, id) = exchanged(Timestamp(0));
        som(&mut c, Timestamp(10), id, ECU, PENDING);
        assert_eq!(c.next_deadline(), None);
        ind(&mut c, Timestamp(20), id, ECU, SResult::Ok, Some(PENDING));
        assert_eq!(c.next_deadline(), Some(Timestamp(5_021)));
    }

    /// ``UDSS_LLR_0144`` — a response-pending message restarts `tP_Client` with the
    /// enhanced reload, and its expiry says so.
    #[test]
    fn a_response_pending_message_opens_the_enhanced_window() {
        let (mut c, id) = exchanged(Timestamp(0));
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(PENDING));
        assert_eq!(c.next_deadline(), Some(Timestamp(5_011)));
        assert_eq!(outputs(c.tick(Timestamp(5_010))).0, NOTHING);
        assert_eq!(
            outputs(c.tick(Timestamp(5_011))).0,
            only(timeout(phys(ECU), ChannelReload::Enhanced))
        );
    }

    /// ``UDSS_LLR_0136``, ``UDSS_LLR_0128`` — a failed reception ends the request and is
    /// indicated.
    #[test]
    fn a_failed_reception_closes_the_window() {
        let (mut c, id) = exchanged(Timestamp(0));
        let out = ind(&mut c, Timestamp(10), id, ECU, FAILED, None);
        assert_eq!(out, only(indicate(id, ECU, FAILED)));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0144`` — only a successful response-pending message opens the
    /// enhanced window; a failed one ends the request like any failed reception.
    #[test]
    fn a_failed_response_pending_reception_opens_no_enhanced_window() {
        let (mut c, id) = exchanged(Timestamp(0));
        ind(&mut c, Timestamp(10), id, ECU, FAILED, Some(PENDING));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0130``, ``UDSS_LLR_0045`` — a completion pairs with the
    /// start-of-message it ends, even once that start's request is over, so it is not a
    /// first indication for the request now in progress.
    #[test]
    fn a_completion_after_the_window_closed_is_still_paired() {
        let (mut c, id) = exchanged(Timestamp(0));
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        exchange(&mut c, Timestamp(20), phys(ECU), UNKNOWN);
        ind(&mut c, Timestamp(30), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
    }

    /// ``UDSS_LLR_0069``, ``UDSS_LLR_0015`` — a successful reception must state its
    /// kind; one that does not is refused and nothing is indicated.
    #[test]
    fn a_successful_reception_stating_no_kind_is_rejected() {
        let (mut c, id) = exchanged(Timestamp(0));
        let (out, ok) =
            outputs(c.t_data_ind(Timestamp(10), id, rsp(ECU), &DATA, SResult::Ok, None));
        assert_eq!(out, NOTHING);
        assert!(rejected(ok, Cause::KindRequired));
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
    }

    /// ``UDSS_LLR_0058`` — a failed reception may omit the kind.
    #[test]
    fn a_failed_reception_may_state_no_kind() {
        let (mut c, id) = exchanged(Timestamp(0));
        ind(&mut c, Timestamp(10), id, ECU, FAILED, None);
    }

    /// ``UDSS_LLR_0027`` — an indication naming no channel the client has is refused.
    #[test]
    fn an_indication_naming_no_channel_is_rejected() {
        let (mut c, id) = exchanged(Timestamp(0));
        assert_eq!(outputs(c.withdraw_channel(Timestamp(1), id)).1, Ok(()));
        let (out, ok) = outputs(c.t_data_ind(
            Timestamp(10),
            id,
            rsp(ECU),
            &DATA,
            SResult::Ok,
            Some(SOLICITED),
        ));
        assert_eq!(out, NOTHING);
        assert!(rejected(ok, Cause::NoSuchChannel));
        let (out, ok) = outputs(c.t_data_som_ind(Timestamp(10), id, rsp(ECU), SOLICITED));
        assert_eq!(out, NOTHING);
        assert!(rejected(ok, Cause::NoSuchChannel));
    }

    /// ``UDSS_LLR_0016`` — one report states a missing channel and a missing kind.
    #[test]
    fn one_report_states_a_missing_channel_and_a_missing_kind() {
        let (mut c, id) = exchanged(Timestamp(0));
        assert_eq!(outputs(c.withdraw_channel(Timestamp(1), id)).1, Ok(()));
        let (_, ok) =
            outputs(c.t_data_ind(Timestamp(10), id, rsp(ECU), &DATA, SResult::Ok, None));
        assert!(rejected(ok, Cause::NoSuchChannel));
        assert!(rejected(ok, Cause::KindRequired));
    }

    /// ``UDSS_LLR_0028``, ``UDSS_LLR_0026`` — the channel the caller names is the one
    /// acted on; it is not checked against the addressing.
    #[test]
    fn the_channel_named_is_trusted() {
        let (mut c, id) = exchanged(Timestamp(0));
        ind(
            &mut c,
            Timestamp(10),
            id,
            ECU_2,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0129``, ``UDSS_LLR_0136`` — a response with no request in progress is
    /// indicated and acts on no timer.
    #[test]
    fn a_response_arriving_with_no_request_acts_on_no_timer() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        let out = ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(out, only(indicate(id, ECU, SResult::Ok)));
        assert_eq!(c.next_deadline(), None);
    }
}

mod functional_window {
    use super::*;

    const ECU_3: u16 = 0x0012;

    fn exchanged(now: Timestamp, class: ClientTx) -> (Tester, FunctionalChannelId) {
        let mut c = tester();
        let id = open_func(&mut c, now);
        exchange(&mut c, now, func(), class);
        (c, id)
    }

    const fn exactly(n: u16) -> ClientTx {
        match NonZeroU16::new(n) {
            Some(n) => request(ExpectedResponses::Exactly(n)),
            None => NO_RESPONSE,
        }
    }

    fn capacity(channel: FunctionalChannelId, sa: u16) -> ClientOutput<'static> {
        ClientOutput::Capacity {
            channel,
            sa: Address(sa),
            ae: None,
        }
    }

    /// ``UDSS_LLR_0137`` — a response restarts a functional window rather than closing
    /// it, since other servers may still answer.
    #[test]
    fn a_response_restarts_the_functional_window() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        ind(&mut c, Timestamp(30), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), Some(Timestamp(81)));
    }

    /// ``UDSS_LLR_0137``, ``UDSS_LLR_0128`` — a failed reception closes it.
    #[test]
    fn a_failed_reception_closes_the_functional_window() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        let out = ind(&mut c, Timestamp(10), id, ECU, FAILED, None);
        assert_eq!(out, only(indicate(id, ECU, FAILED)));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0138`` — the response that completes an exact count closes the
    /// window silently.
    #[test]
    fn the_last_expected_response_closes_the_window() {
        let (mut c, id) = exchanged(Timestamp(0), exactly(2));
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), Some(Timestamp(61)));
        ind(
            &mut c,
            Timestamp(20),
            id,
            ECU_2,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(c.next_deadline(), None);
        assert_eq!(outputs(c.tick(Timestamp(1_000))).0, NOTHING);
    }

    /// ``UDSS_LLR_0138``, ``UDSS_LLR_0137`` — an unsolicited response neither counts nor
    /// restarts the window.
    #[test]
    fn an_unsolicited_response_is_not_counted() {
        let (mut c, id) = exchanged(Timestamp(0), request(ONE));
        ind(
            &mut c,
            Timestamp(10),
            id,
            ECU,
            SResult::Ok,
            Some(UNSOLICITED),
        );
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
        ind(&mut c, Timestamp(20), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0137`` — a response-pending start-of-message restarts a functional
    /// window, where on a physical channel it stops it.
    #[test]
    fn a_response_pending_start_restarts_rather_than_stops() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, PENDING);
        assert_eq!(c.next_deadline(), Some(Timestamp(61)));
    }

    /// ``UDSS_LLR_0145``, ``UDSS_LLR_0146``, ``UDSS_LLR_0144`` — while one responder's
    /// response-pending message is outstanding, another's response restarts the window
    /// with the enhanced reload.
    #[test]
    fn a_responders_response_pending_puts_the_enhanced_reload_in_force() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(PENDING));
        assert_eq!(c.next_deadline(), Some(Timestamp(5_011)));
        som(&mut c, Timestamp(20), id, ECU_2, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(5_021)));
    }

    /// ``UDSS_LLR_0146``, ``UDSS_LLR_0147`` — the responder's next message ends its
    /// pending fact before the reload in force is read.
    #[test]
    fn its_next_message_ends_the_pending_fact_before_the_reload_is_read() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(PENDING));
        som(&mut c, Timestamp(20), id, ECU, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
    }

    /// ``UDSS_LLR_0147`` — a further response-pending message ends the first and records
    /// itself, and the record stands.
    #[test]
    fn a_repeated_response_pending_message_stays_outstanding() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(PENDING));
        ind(&mut c, Timestamp(20), id, ECU, SResult::Ok, Some(PENDING));
        assert_eq!(c.next_deadline(), Some(Timestamp(5_021)));
        som(&mut c, Timestamp(30), id, ECU_2, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(5_031)));
    }

    /// ``UDSS_LLR_0045``, ``UDSS_LLR_0139`` — interleaved multi-frame responses pair by
    /// responder, so neither completion is a first indication.
    #[test]
    fn interleaved_multi_frame_responses_pair_by_responder() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(61)));
        som(&mut c, Timestamp(20), id, ECU_2, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
        ind(&mut c, Timestamp(30), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
        ind(
            &mut c,
            Timestamp(40),
            id,
            ECU_2,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
    }

    /// ``UDSS_LLR_0143`` — a response-pending message from a responder the full table
    /// has no room for is reported, ahead of its indication.
    #[test]
    fn a_responder_beyond_capacity_is_reported_before_its_indication() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        som(&mut c, Timestamp(10), id, ECU_2, SOLICITED);
        let out = ind(&mut c, Timestamp(20), id, ECU_3, SResult::Ok, Some(PENDING));
        assert_eq!(
            out,
            [
                Some(capacity(id, ECU_3)),
                Some(indicate(id, ECU_3, SResult::Ok)),
                None,
                None,
                None,
                None,
            ]
        );
    }

    /// ``UDSS_LLR_0143`` — an untracked responder's start-of-message is reported and is a
    /// first indication, and so is its completion.
    #[test]
    fn an_untracked_responders_completion_is_a_first_indication() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        som(&mut c, Timestamp(10), id, ECU_2, SOLICITED);
        let out = som(&mut c, Timestamp(20), id, ECU_3, SOLICITED);
        assert_eq!(out, only(capacity(id, ECU_3)));
        assert_eq!(c.next_deadline(), Some(Timestamp(71)));
        let out = ind(
            &mut c,
            Timestamp(30),
            id,
            ECU_3,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(out, only(indicate(id, ECU_3, SResult::Ok)));
        assert_eq!(c.next_deadline(), Some(Timestamp(81)));
    }

    /// ``UDSS_LLR_0141`` — when the request ends, pending facts clear and entries whose
    /// start-of-message is open stay until completed.
    #[test]
    fn at_the_end_of_the_request_pending_facts_clear_and_open_entries_stay() {
        let (mut c, id) = exchanged(Timestamp(0), UNKNOWN);
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(PENDING));
        som(&mut c, Timestamp(20), id, ECU_2, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(5_021)));
        assert_eq!(
            outputs(c.tick(Timestamp(5_021))).0,
            only(timeout(func(), ChannelReload::Enhanced))
        );
        ind(
            &mut c,
            Timestamp(5_030),
            id,
            ECU_2,
            SResult::Ok,
            Some(SOLICITED),
        );
        exchange(&mut c, Timestamp(5_100), func(), UNKNOWN);
        som(&mut c, Timestamp(5_110), id, ECU, SOLICITED);
        assert_eq!(c.next_deadline(), Some(Timestamp(5_161)));
    }

    /// ``UDSS_LLR_0142`` — a functional channel opens with an empty table, whatever the
    /// slot's last channel left in it.
    #[test]
    fn a_fresh_functional_channel_has_an_empty_table() {
        let (mut c, old) = exchanged(Timestamp(0), UNKNOWN);
        som(&mut c, Timestamp(10), old, ECU, SOLICITED);
        som(&mut c, Timestamp(10), old, ECU_2, SOLICITED);
        assert_eq!(outputs(c.withdraw_channel(Timestamp(20), old)).1, Ok(()));
        let id = open_func(&mut c, Timestamp(20));
        assert_eq!(som(&mut c, Timestamp(30), id, ECU, SOLICITED), NOTHING);
        assert_eq!(som(&mut c, Timestamp(30), id, ECU_2, SOLICITED), NOTHING);
        assert_eq!(
            som(&mut c, Timestamp(30), id, ECU_3, SOLICITED),
            only(capacity(id, ECU_3))
        );
    }

    /// ``UDSS_LLR_0139`` — a physical channel keeps no table, so it has none to fill.
    #[test]
    fn a_physical_channel_reports_no_capacity() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        for sa in [ECU, ECU_2, ECU_3, 0x0013] {
            assert_eq!(som(&mut c, Timestamp(10), id, sa, SOLICITED), NOTHING);
        }
    }
}

mod spacing {
    use super::*;

    fn spaced_func(c: &mut Tester) -> FunctionalChannelId {
        open_func_with(c, Timestamp(0), SPACED_FUNC)
    }

    fn remaining(outcome: Result<(), Rejection>) -> Option<u32> {
        outcome
            .err()?
            .causes()
            .find_map(|reported| match reported.content {
                Some(Content::SpacingTimerRunning { remaining }) => Some(remaining),
                _ => None,
            })
    }

    fn send(c: &mut Tester, now: u32, ai: Ai, class: ClientTx) -> Result<(), Rejection> {
        outputs(c.s_data_req(Timestamp(now), ai, &DATA, class)).1
    }

    /// ``UDSS_LLR_0169``, ``UDSS_LLR_0166``, ``UDSS_LLR_0171`` — a confirmed physical
    /// request expecting no response starts the spacing timer, which refuses a request
    /// until it reaches its value.
    #[test]
    fn a_confirmed_physical_request_needing_no_response_starts_spacing() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), NO_RESPONSE);
        assert_eq!(c.next_deadline(), Some(Timestamp(60)));
        let early = send(&mut c, 59, phys(ECU), UNKNOWN);
        assert!(rejected(early, Cause::SpacingTimerRunning));
        assert_eq!(remaining(early), Some(1));
        assert_eq!(send(&mut c, 60, phys(ECU), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0169`` — a physical request expecting a response starts none.
    #[test]
    fn a_physical_request_needing_a_response_starts_no_spacing() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        ind(&mut c, Timestamp(10), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), None);
        assert_eq!(send(&mut c, 11, phys(ECU), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0169`` — a failed physical transmission starts it, whatever the
    /// request expected.
    #[test]
    fn a_failed_physical_transmission_starts_spacing() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(send(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        let (_, confirmed) = outputs(c.t_data_conf(Timestamp(0), phys(ECU), FAILED));
        assert_eq!(confirmed, Ok(()));
        assert_eq!(c.next_deadline(), Some(Timestamp(60)));
    }

    /// ``UDSS_LLR_0170``, ``UDSS_LLR_0172`` — every functional confirmation starts it,
    /// and a refusal states the time left.
    #[test]
    fn any_functional_confirmation_starts_spacing() {
        let mut c = tester();
        spaced_func(&mut c);
        exchange(&mut c, Timestamp(0), func(), UNKNOWN);
        assert_eq!(c.next_deadline(), Some(Timestamp(51)));
        assert_ne!(outputs(c.tick(Timestamp(51))).0, NOTHING);
        assert_eq!(c.next_deadline(), Some(Timestamp(70)));
        assert_eq!(remaining(send(&mut c, 52, func(), UNKNOWN)), Some(18));
    }

    /// ``UDSS_LLR_0170`` — a failed functional transmission starts it too.
    #[test]
    fn a_failed_functional_transmission_starts_spacing() {
        let mut c = tester();
        spaced_func(&mut c);
        assert_eq!(send(&mut c, 0, func(), UNKNOWN), Ok(()));
        let (_, confirmed) = outputs(c.t_data_conf(Timestamp(0), func(), FAILED));
        assert_eq!(confirmed, Ok(()));
        assert_eq!(c.next_deadline(), Some(Timestamp(70)));
    }

    /// ``UDSS_LLR_0172`` — the time left is the loaded value less the time elapsed, and
    /// nothing is sent.
    #[test]
    fn the_rejection_states_the_time_remaining() {
        let mut c = tester();
        spaced_func(&mut c);
        exchange(&mut c, Timestamp(0), func(), NO_RESPONSE);
        let (out, refused) = outputs(c.s_data_req(Timestamp(25), func(), &DATA, UNKNOWN));
        assert_eq!(out, NOTHING);
        assert_eq!(remaining(refused), Some(45));
    }

    /// ``UDSS_LLR_0164``, ``UDSS_LLR_0171`` — the timer is the channel's own.
    #[test]
    fn the_spacing_timer_is_per_channel() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        open_phys(&mut c, Timestamp(0), ECU_2);
        exchange(&mut c, Timestamp(0), phys(ECU), NO_RESPONSE);
        assert_eq!(send(&mut c, 10, phys(ECU_2), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0171`` — a keep-alive is a request like any other.
    #[test]
    fn a_keep_alive_is_spaced_like_any_request() {
        let mut c = tester();
        spaced_func(&mut c);
        exchange(&mut c, Timestamp(0), func(), NO_RESPONSE);
        let keep_alive = ClientTx::KeepAlive {
            expected: ExpectedResponses::None,
        };
        assert!(rejected(
            send(&mut c, 10, func(), keep_alive),
            Cause::SpacingTimerRunning
        ));
    }

    /// ``UDSS_LLR_0169`` — each confirmation starts the timer afresh.
    #[test]
    fn each_confirmation_starts_the_spacing_timer_afresh() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), NO_RESPONSE);
        exchange(&mut c, Timestamp(60), phys(ECU), NO_RESPONSE);
        assert_eq!(c.next_deadline(), Some(Timestamp(120)));
    }

    /// ``UDSS_LLR_0166``, ``UDSS_LLR_0168`` — the expiry stops the timer and does nothing
    /// else.
    #[test]
    fn spacing_expiry_does_nothing_else() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), NO_RESPONSE);
        assert_eq!(outputs(c.tick(Timestamp(60))).0, NOTHING);
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0019`` — a spacing wait begun just before the wrap ends just after it.
    #[test]
    fn the_spacing_wait_runs_across_the_wrap() {
        let mut c = tester();
        let start = Timestamp(u32::MAX - 9);
        open_phys(&mut c, start, ECU);
        exchange(&mut c, start, phys(ECU), NO_RESPONSE);
        assert_eq!(c.next_deadline(), Some(Timestamp(50)));
        assert_eq!(remaining(send(&mut c, 49, phys(ECU), UNKNOWN)), Some(1));
        assert_eq!(send(&mut c, 50, phys(ECU), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0080``, ``UDSS_LLR_0019`` — of a deadline before the wrap and one after
    /// it, the one before is reported, though it is the larger number.
    #[test]
    fn the_earlier_of_two_deadlines_is_reported_across_the_wrap() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        open_phys(&mut c, Timestamp(0), ECU_2);
        exchange(&mut c, Timestamp(u32::MAX - 51), phys(ECU_2), UNKNOWN);
        exchange(&mut c, Timestamp(u32::MAX - 10), phys(ECU), NO_RESPONSE);
        assert_eq!(c.next_deadline(), Some(Timestamp(u32::MAX)));
    }
}

mod error_handling {
    use super::*;

    const REPEAT: ClientTx = ClientTx::Request {
        expected: ExpectedResponses::Unknown,
        repeat: true,
        session: None,
    };
    const KEEP_ALIVE: ClientTx = ClientTx::KeepAlive {
        expected: ExpectedResponses::None,
    };

    fn send(c: &mut Tester, now: u32, ai: Ai, class: ClientTx) -> Result<(), Rejection> {
        outputs(c.s_data_req(Timestamp(now), ai, &DATA, class)).1
    }

    /// One transmission that failed: its confirmation starts the spacing timer and no
    /// window, as a request the client goes on to repeat.
    fn attempt(c: &mut Tester, now: u32, ai: Ai, class: ClientTx) -> Result<(), Rejection> {
        send(c, now, ai, class)?;
        outputs(c.t_data_conf(Timestamp(now), ai, FAILED)).1
    }

    fn reset(
        c: &mut Tester,
        now: u32,
        channel: impl Into<ChannelId>,
    ) -> Result<(), Rejection> {
        let (out, outcome) = outputs(c.reset_channel(Timestamp(now), channel));
        assert_eq!(out, NOTHING);
        outcome
    }

    /// ``UDSS_LLR_0177``, ``UDSS_LLR_0176``, ``UDSS_LLR_0173``, ``UDSS_LLR_0174`` — two
    /// repeats follow a request; a third is refused.
    #[test]
    fn a_third_repeat_is_rejected() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(attempt(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        assert_eq!(attempt(&mut c, 60, phys(ECU), REPEAT), Ok(()));
        assert_eq!(attempt(&mut c, 120, phys(ECU), REPEAT), Ok(()));
        let third = send(&mut c, 180, phys(ECU), REPEAT);
        assert!(rejected(third, Cause::RepeatCountSpent));
    }

    /// ``UDSS_LLR_0176`` — a request not marked repeat starts the count again.
    #[test]
    fn an_unmarked_request_resets_the_count() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        for (now, class) in [
            (0, UNKNOWN),
            (60, REPEAT),
            (120, REPEAT),
            (180, UNKNOWN),
            (240, REPEAT),
            (300, REPEAT),
        ] {
            assert_eq!(attempt(&mut c, now, phys(ECU), class), Ok(()));
        }
    }

    /// ``UDSS_LLR_0176`` — a keep-alive leaves the count where it was.
    #[test]
    fn a_keep_alive_leaves_the_count_alone() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        for (now, class) in [(0, UNKNOWN), (60, REPEAT), (120, REPEAT), (180, KEEP_ALIVE)] {
            assert_eq!(attempt(&mut c, now, phys(ECU), class), Ok(()));
        }
        let repeat = send(&mut c, 240, phys(ECU), REPEAT);
        assert!(rejected(repeat, Cause::RepeatCountSpent));
    }

    /// ``UDSS_LLR_0176``, ``UDSS_LLR_0015`` — a refused repeat is not counted.
    #[test]
    fn a_rejected_repeat_is_not_counted() {
        let mut c = tester();
        open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(attempt(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        let early = send(&mut c, 30, phys(ECU), REPEAT);
        assert!(rejected(early, Cause::SpacingTimerRunning));
        assert_eq!(attempt(&mut c, 60, phys(ECU), REPEAT), Ok(()));
        assert_eq!(attempt(&mut c, 120, phys(ECU), REPEAT), Ok(()));
    }

    /// ``UDSS_LLR_0178``, ``UDSS_LLR_0141`` — after the window, a functional request
    /// waits for every response still arriving.
    #[test]
    fn a_functional_request_waits_for_responses_still_arriving() {
        let mut c = tester();
        let id = open_func(&mut c, Timestamp(0));
        exchange(&mut c, Timestamp(0), func(), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        assert_eq!(
            outputs(c.tick(Timestamp(61))).0,
            only(timeout(func(), ChannelReload::Default))
        );
        let waiting = send(&mut c, 200, func(), UNKNOWN);
        assert!(rejected(waiting, Cause::ResponseStillArriving));
        ind(
            &mut c,
            Timestamp(210),
            id,
            ECU,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(send(&mut c, 220, func(), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0140``, ``UDSS_LLR_0178`` — a start-of-message arriving with no request
    /// in progress creates an entry, which holds the next request back.
    #[test]
    fn a_start_of_message_after_the_window_creates_an_entry() {
        let mut c = tester();
        let id = open_func(&mut c, Timestamp(0));
        exchange(&mut c, Timestamp(0), func(), UNKNOWN);
        assert_ne!(outputs(c.tick(Timestamp(51))).0, NOTHING);
        som(&mut c, Timestamp(60), id, ECU, SOLICITED);
        let waiting = send(&mut c, 200, func(), UNKNOWN);
        assert!(rejected(waiting, Cause::ResponseStillArriving));
    }

    /// ``UDSS_LLR_0179``, ``UDSS_LLR_0016``, ``UDSS_LLR_0015`` — a refused repeat states
    /// every cause that held, and changes nothing.
    #[test]
    fn a_rejected_repeat_states_every_cause_and_changes_nothing() {
        let mut c = tester();
        let id = open_func_with(&mut c, Timestamp(0), SPACED_FUNC);
        assert_eq!(attempt(&mut c, 0, func(), UNKNOWN), Ok(()));
        assert_eq!(attempt(&mut c, 70, func(), REPEAT), Ok(()));
        assert_eq!(attempt(&mut c, 140, func(), REPEAT), Ok(()));
        som(&mut c, Timestamp(150), id, ECU, SOLICITED);
        let before = c.next_deadline();
        let (out, refused) = outputs(c.s_data_req(Timestamp(160), func(), &DATA, REPEAT));
        assert_eq!(out, NOTHING);
        assert!(rejected(refused, Cause::RepeatCountSpent));
        assert!(rejected(refused, Cause::ResponseStillArriving));
        assert_eq!(
            refused
                .err()
                .map(|r| r.causes().filter_map(|c| c.content).count()),
            Some(1)
        );
        assert!(rejected(refused, Cause::SpacingTimerRunning));
        assert_eq!(c.next_deadline(), before);
        ind(
            &mut c,
            Timestamp(170),
            id,
            ECU,
            SResult::Ok,
            Some(SOLICITED),
        );
        assert_eq!(send(&mut c, 210, func(), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0180``, ``UDSS_LLR_0128`` — a reset ends the request and its window
    /// without indicating anything.
    #[test]
    fn a_reset_ends_the_request_silently() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        assert_eq!(reset(&mut c, 10, id), Ok(()));
        assert_eq!(c.next_deadline(), None);
        assert_eq!(outputs(c.tick(Timestamp(100))).0, NOTHING);
    }

    /// ``UDSS_LLR_0180``, ``UDSS_LLR_0168`` — a reset leaves the spacing timer running.
    #[test]
    fn a_reset_leaves_the_spacing_timer_running() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), NO_RESPONSE);
        assert_eq!(reset(&mut c, 10, id), Ok(()));
        let early = send(&mut c, 20, phys(ECU), UNKNOWN);
        assert!(rejected(early, Cause::SpacingTimerRunning));
    }

    /// ``UDSS_LLR_0180`` — a reset zeroes the repeat count.
    #[test]
    fn a_reset_zeroes_the_repeat_count() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(attempt(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        assert_eq!(attempt(&mut c, 60, phys(ECU), REPEAT), Ok(()));
        assert_eq!(attempt(&mut c, 120, phys(ECU), REPEAT), Ok(()));
        assert_eq!(reset(&mut c, 150, id), Ok(()));
        assert_eq!(attempt(&mut c, 180, phys(ECU), REPEAT), Ok(()));
        assert_eq!(attempt(&mut c, 240, phys(ECU), REPEAT), Ok(()));
    }

    /// ``UDSS_LLR_0180``, ``UDSS_LLR_0130`` — a reset closes a physical channel's open
    /// start-of-message, so the next completion is a first indication.
    #[test]
    fn a_reset_closes_the_open_start_of_message() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        exchange(&mut c, Timestamp(0), phys(ECU), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        assert_eq!(reset(&mut c, 20, id), Ok(()));
        exchange(&mut c, Timestamp(30), phys(ECU), UNKNOWN);
        ind(&mut c, Timestamp(40), id, ECU, SResult::Ok, Some(SOLICITED));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0180`` — a reset releases every entry of a functional channel's table.
    #[test]
    fn a_reset_releases_every_responder_entry() {
        let mut c = tester();
        let id = open_func(&mut c, Timestamp(0));
        exchange(&mut c, Timestamp(0), func(), UNKNOWN);
        som(&mut c, Timestamp(10), id, ECU, SOLICITED);
        assert_eq!(reset(&mut c, 20, id), Ok(()));
        assert_eq!(send(&mut c, 30, func(), UNKNOWN), Ok(()));
    }

    /// ``UDSS_LLR_0181`` — an association the reset abandoned stays outstanding.
    #[test]
    fn an_abandoned_association_stays_outstanding() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(send(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        assert_eq!(reset(&mut c, 10, id), Ok(()));
        let again = send(&mut c, 20, phys(ECU), UNKNOWN);
        assert!(rejected(again, Cause::AssociationOutstanding));
    }

    /// ``UDSS_LLR_0182`` — its confirmation is forwarded and opens no window.
    #[test]
    fn an_abandoned_confirmation_is_forwarded_and_opens_no_window() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(send(&mut c, 0, phys(ECU), UNKNOWN), Ok(()));
        assert_eq!(reset(&mut c, 10, id), Ok(()));
        let (out, confirmed) =
            outputs(c.t_data_conf(Timestamp(20), phys(ECU), SResult::Ok));
        assert_eq!(
            out,
            only(ClientOutput::Confirm {
                ai: phys(ECU),
                result: SResult::Ok,
            })
        );
        assert_eq!(confirmed, Ok(()));
        assert_eq!(c.next_deadline(), None);
    }

    /// ``UDSS_LLR_0182``, ``UDSS_LLR_0169`` — and otherwise acts as it would have, so it
    /// still starts the spacing timer.
    #[test]
    fn an_abandoned_confirmation_still_starts_spacing() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(send(&mut c, 0, phys(ECU), NO_RESPONSE), Ok(()));
        assert_eq!(reset(&mut c, 10, id), Ok(()));
        let (_, confirmed) = outputs(c.t_data_conf(Timestamp(20), phys(ECU), SResult::Ok));
        assert_eq!(confirmed, Ok(()));
        assert_eq!(c.next_deadline(), Some(Timestamp(80)));
    }

    /// ``UDSS_LLR_0183`` — a reset naming no channel the client has is refused.
    #[test]
    fn a_reset_naming_no_channel_is_rejected() {
        let mut c = tester();
        let id = open_phys(&mut c, Timestamp(0), ECU);
        assert_eq!(outputs(c.withdraw_channel(Timestamp(0), id)).1, Ok(()));
        assert!(rejected(reset(&mut c, 10, id), Cause::NoSuchChannel));
    }
}
