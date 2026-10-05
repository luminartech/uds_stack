//! Behavioural tests of the client role, one per requirement a body satisfies.

use core::num::NonZeroU16;

use uds_session::{
    Address, Ai, Cause, ChannelAddressing, ChannelId, ChannelParameter, ChannelParams,
    ChannelReload, Client, ClientOutput, ClientReaction, ClientRx, ClientTx,
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
const FUNC_PARAMS: ChannelParams = ChannelParams {
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
#[allow(dead_code, reason = "used by the response tests")]
const ONE: ExpectedResponses = ExpectedResponses::Exactly(NonZeroU16::MIN);
#[allow(dead_code, reason = "used by the response tests")]
const SOLICITED: ClientRx = ClientRx::FinalResponse {
    solicitation: Solicitation::Solicited,
    session: None,
};

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
fn open_func<K: KeepAliveMode, const P: usize, const F: usize, const R: usize>(
    c: &mut Client<K, P, F, R>,
    now: Timestamp,
) -> FunctionalChannelId {
    let (_, id) = outputs(c.open_functional_channel(now, to(FUNCTIONAL), FUNC_PARAMS));
    id.unwrap_or_else(|r| panic!("open failed: {r}"))
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
        let failed = SResult::Transport(TransportError(1));
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
