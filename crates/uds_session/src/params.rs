//! Protocol parameters.
//!
//! ``UDSS_LLR_0041`` makes every timing parameter a 32-bit value in the timestamp's unit,
//! so each field below is a `u32` of milliseconds. ``UDSS_LLR_0042`` gives none of them a
//! default — the recommended values in ISO 14229-2:2021 clause 9 are properties of a
//! vehicle network, not of this crate — which is why they are supplied at creation rather
//! than defaulted. ``UDSS_LLR_0043`` then allows any of them to be set again at any time.

/// Which reload value a server's response timer was carrying.
///
/// ``UDSS_LLR_0117`` has the overrun indication state it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerReload {
    /// `tP2_Server_Max`.
    P2,
    /// `tP2*_Server_Max`, the enhanced window.
    P2Star,
}

/// Which reload value a channel's response timer was carrying.
///
/// ``UDSS_LLR_0132`` names the pair; ``UDSS_LLR_0148`` has the timeout indication state
/// which was in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelReload {
    /// `tP2_Client_Max`, or `tP6_Client_Max` on a transport without `T_DataSOM.ind`.
    Default,
    /// `tP2*_Client_Max`, or `tP6*_Client_Max`, the enhanced window.
    Enhanced,
}

/// What creating a server supplies.
///
/// ``UDSS_LLR_0032`` with ``UDSS_LLR_0042``. There is deliberately no spacing parameter:
/// ``UDSS_LLR_0119`` derives the minimum spacing between response-pending messages as the
/// least whole millisecond not less than three tenths of `p2_star_server_max`, read as
/// that parameter stands at the time, so a parameter for it could only disagree.
///
/// These are what the timers enforce. A `DiagnosticSessionControl` positive response
/// advertises `P2Server_max` and `P2*Server_max` (ISO 14229-1:2020 Table 29); keeping the
/// advertised values equal to these is the obligation of whatever composes that response,
/// and [`Server::set_parameter`] is how a caller changes them afterwards.
///
/// [`Server::set_parameter`]: crate::Server::set_parameter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerParams {
    /// `tS3_Server` — how long a non-default session survives without a request.
    pub s3_server: u32,
    /// `tP2_Server_Max` — the default response window.
    pub p2_server_max: u32,
    /// `tP2*_Server_Max` — the enhanced response window.
    pub p2_star_server_max: u32,
    /// How long before `tP2_Server` closes its overrun is indicated (``UDSS_LLR_0186``):
    /// the server's own latency from that indication to a response-pending message
    /// leaving, the server-side counterpart of a client's `ΔP2`.
    ///
    /// ISO 14229-2:2021 9.2 Table 3 makes `tP2_Server` and `tP2*_Server` performance
    /// requirements, and 10.1.3 Figure 11 note d has the response-pending message sent
    /// "within `tP2_Server`"; Table 7 runs the server's timer "to ensure that subsequent
    /// … 78 are transmitted prior to the expired `tP2*_Server`". An indication at the
    /// boundary leaves the message to go out after it, so ``UDSS_LLR_0117`` delivers it
    /// this much earlier. The timer is still loaded with the full window
    /// (``UDSS_LLR_0113``, ``UDSS_LLR_0116``), and the overrun names that window.
    ///
    /// Zero indicates at the boundary itself. The value is taken when the window opens,
    /// so a change moves no window already open (``UDSS_LLR_0043``). A lead not less than
    /// `p2_server_max` indicates every overrun on the first timestamp after the request,
    /// and one above seven tenths of `p2_star_server_max` indicates the enhanced overrun
    /// before ``UDSS_LLR_0119`` admits the next response-pending message;
    /// [`ServerParams::is_well_formed`] checks both.
    pub response_pending_lead: u32,
}

impl ServerParams {
    /// Whether the response-pending lead fits both windows (``UDSS_LLR_0186``).
    ///
    /// It must be less than `p2_server_max`, so that the overrun of the default window is
    /// not indicated on the request's own reception, and no more than `p2_star_server_max`
    /// less ``UDSS_LLR_0119``'s spacing — ⌊0,7 × `p2_star_server_max`⌋ — so that the
    /// enhanced overrun is indicated when a further response-pending message is
    /// admissible. Construction does not check this: [`crate::Server::new`] is a `const
    /// fn` used in `static`s and stays infallible, so an assembly asserts it where its
    /// parameters are fixed.
    ///
    /// ```
    /// use uds_session::ServerParams;
    ///
    /// const PARAMS: ServerParams = ServerParams {
    ///     s3_server: 5_000,
    ///     p2_server_max: 50,
    ///     p2_star_server_max: 5_000,
    ///     response_pending_lead: 10,
    /// };
    /// const { assert!(PARAMS.is_well_formed()) };
    /// ```
    #[must_use]
    pub const fn is_well_formed(&self) -> bool {
        let lead = self.response_pending_lead;
        let enhanced = self
            .p2_star_server_max
            .saturating_sub(self.response_pending_spacing());
        lead < self.p2_server_max && lead <= enhanced
    }

    /// ``UDSS_LLR_0119`` — ⌈3 × `tP2*_Server_Max` / 10⌉ in integer arithmetic, from the
    /// parameter as it stands.
    pub(crate) const fn response_pending_spacing(&self) -> u32 {
        let p = self.p2_star_server_max;
        let q = p / 10;
        let r = p % 10;
        q.saturating_mul(3)
            .saturating_add(r.saturating_mul(3).saturating_add(9) / 10)
    }
}

/// One server parameter, for setting it again.
///
/// ``UDSS_LLR_0040`` puts parameter setting in the service interface; ``UDSS_LLR_0043``
/// permits it at any time, and ``UDSS_LLR_0076`` keeps a running timer on the value it
/// was loaded with, so a change never moves a window already open.
///
/// Exhaustive. ``UDSS_LLR_0041`` makes the protocol parameters exactly those a
/// requirement of this set conditions on, so the set is closed by the requirement set
/// rather than open-ended. A parameter can only appear by a requirement changing, and a
/// caller whose match then fails to compile is being told exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerParameter {
    /// `tS3_Server`.
    S3Server(u32),
    /// `tP2_Server_Max`.
    P2ServerMax(u32),
    /// `tP2*_Server_Max`.
    P2StarServerMax(u32),
    /// The response-pending lead of ``UDSS_LLR_0186``.
    ResponsePendingLead(u32),
}

/// The `tP_Client` reload pair a transport dictates.
///
/// ``UDSS_LLR_0132`` names the pair. They are nested rather than flat because a
/// transport dictates them while having no view on the `tP3` spacing of
/// ``UDSS_LLR_0165``, which is client policy under ISO 14229-2:2021 9.7; the split
/// follows the requirement boundary rather than cutting across it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reloads {
    /// ``UDSS_LLR_0132`` — `tP2_Client_Max`, or `tP6_Client_Max` where the transport has
    /// no `T_DataSOM.ind`. The session layer does not distinguish the two cases.
    pub default_reload: u32,
    /// ``UDSS_LLR_0132`` — `tP2*_Client_Max` or `tP6*_Client_Max`.
    pub enhanced_reload: u32,
}

impl Reloads {
    /// The reload `which` names.
    ///
    /// ``UDSS_LLR_0132`` defines the pair; ``UDSS_LLR_0148`` has a timeout indication
    /// name which of the two was in force. The mapping between them is ISO
    /// 14229-2:2021 and nothing else, so it belongs here rather than in a caller that
    /// holds both a `Reloads` and a `ChannelReload`.
    #[must_use]
    pub const fn value_for(self, which: ChannelReload) -> u32 {
        match which {
            ChannelReload::Default => self.default_reload,
            ChannelReload::Enhanced => self.enhanced_reload,
        }
    }
}

/// What opening a channel supplies.
///
/// ``UDSS_LLR_0126`` holds these with the channel. One type serves both kinds: the two
/// differ only in which ``UDSS_LLR_0165`` spacing parameter the value is, and the opening
/// method says which kind is being opened.
///
/// There is no `tS3_Client` here. ``UDSS_LLR_0152`` gives a physical channel its own
/// reload in physical keep-alive and none at all in functional keep-alive, so the reload
/// is an argument of [`crate::Client::open_physical_channel`] in the mode that has one and
/// absent from the method in the mode that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelParams {
    /// ``UDSS_LLR_0132`` — the response window pair.
    pub reloads: Reloads,
    /// ``UDSS_LLR_0165`` — `tP3_Client_Phys` on a physical channel, `tP3_Client_Func` on
    /// a functional one.
    pub spacing: u32,
}

/// One channel parameter, for setting it again.
///
/// ``UDSS_LLR_0043``; ``UDSS_LLR_0134`` rejects a setting naming a channel the client
/// does not have. One type serves both kinds: the setter takes that kind's own channel
/// identity, which is what keeps a setting from naming a channel of the wrong kind, so the
/// parameter itself need not be split as well.
///
/// `tS3_Client` is not among these. ``UDSS_LLR_0152`` gives a physical channel one only in
/// physical keep-alive, so [`crate::Client::set_physical_s3_client`] carries it and exists
/// only in that mode.
///
/// Exhaustive, for the reason [`ServerParameter`] gives: ``UDSS_LLR_0041`` closes the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelParameter {
    /// Both response reloads at once.
    ///
    /// ``UDSS_LLR_0132`` makes the two a pair a transport dictates together, and
    /// [`Reloads`] is the shape a transport hands over. Setting the pair is one act, so
    /// applying a new transport profile does not leave the channel briefly holding one
    /// reload from each — a state no profile describes.
    Reloads(Reloads),
    /// The default response reload, on its own.
    DefaultReload(u32),
    /// The enhanced response reload, on its own.
    EnhancedReload(u32),
    /// The request spacing.
    ///
    /// ``UDSS_LLR_0165`` makes this client policy rather than transport, which is why it
    /// is not part of [`ChannelParameter::Reloads`].
    Spacing(u32),
}

#[cfg(test)]
mod tests {
    use super::{ChannelParameter, ChannelParams, ChannelReload, Reloads, ServerParams};

    /// ``UDSS_LLR_0151`` and ``UDSS_LLR_0152`` — a `tS3_Client` belongs to a physical
    /// channel in physical keep-alive and to no channel at all in functional keep-alive.
    /// The types carry that: there is no field here on which to state one, in either
    /// kind, so the reload can only arrive through the opening method of the mode that
    /// gives it a meaning.
    #[test]
    fn no_channel_parameter_carries_a_session_reload() {
        let reloads = Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        };
        let params = ChannelParams {
            reloads,
            spacing: 60,
        };
        assert_eq!(params.reloads.default_reload, 50);
        assert_eq!(params.spacing, 60);
    }

    /// ``UDSS_LLR_0119`` derives the response-pending spacing from `tP2*_Server_Max`
    /// rather than taking a parameter, so `ServerParams` has no field for it.
    #[test]
    fn the_server_has_no_spacing_parameter() {
        let p = ServerParams {
            s3_server: 5_000,
            p2_server_max: 50,
            p2_star_server_max: 5_000,
            response_pending_lead: 0,
        };
        assert_eq!(p.p2_star_server_max, 5_000);
    }

    /// ``UDSS_LLR_0186`` — the lead must be less than `tP2_Server_Max` and no more than
    /// `tP2*_Server_Max` less ``UDSS_LLR_0119``'s spacing.
    #[test]
    fn the_lead_is_bounded_by_both_windows() {
        let with =
            |p2_server_max, p2_star_server_max, response_pending_lead| ServerParams {
                s3_server: 5_000,
                p2_server_max,
                p2_star_server_max,
                response_pending_lead,
            };
        assert!(with(50, 5_000, 0).is_well_formed());
        assert!(with(50, 5_000, 49).is_well_formed());
        assert!(!with(50, 5_000, 50).is_well_formed());
        // Spacing of 5001 is 1501, so the lead may be at most 3500.
        assert!(with(5_000, 5_001, 3_500).is_well_formed());
        assert!(!with(5_000, 5_001, 3_501).is_well_formed());
        assert!(!with(0, 5_000, 0).is_well_formed());
        assert!(!with(u32::MAX, u32::MAX, u32::MAX).is_well_formed());
    }

    /// ``UDSS_LLR_0132`` makes the reloads a pair a transport dictates together, so a
    /// caller holding a [`Reloads`] can set it as one parameter rather than taking it
    /// apart into two settings with a mismatched pair in between.
    #[test]
    fn the_reload_pair_can_be_set_as_one_parameter() {
        let reloads = Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        };
        let carried = match ChannelParameter::Reloads(reloads) {
            ChannelParameter::Reloads(set) => Some(set),
            _ => None,
        };
        assert_eq!(carried, Some(reloads));
        assert_eq!(
            carried.map(|set| set.value_for(ChannelReload::Default)),
            Some(50)
        );
        assert_eq!(
            carried.map(|set| set.value_for(ChannelReload::Enhanced)),
            Some(5_000)
        );
    }

    /// ``UDSS_LLR_0132`` names the pair and ``UDSS_LLR_0148`` names which reload a
    /// timeout indication carries, so `value_for` must pick the matching field rather
    /// than a fixed one.
    #[test]
    fn value_for_selects_the_reload_named_by_the_channel_reload() {
        let reloads = Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        };
        assert_eq!(reloads.value_for(ChannelReload::Default), 50);
        assert_eq!(reloads.value_for(ChannelReload::Enhanced), 5_000);
    }
}
