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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerParams {
    /// `tS3_Server` — how long a non-default session survives without a request.
    pub s3_server: u32,
    /// `tP2_Server_Max` — the default response window.
    pub p2_server_max: u32,
    /// `tP2*_Server_Max` — the enhanced response window.
    pub p2_star_server_max: u32,
}

/// One server parameter, for setting it again.
///
/// ``UDSS_LLR_0040`` puts parameter setting in the service interface; ``UDSS_LLR_0043``
/// permits it at any time, and ``UDSS_LLR_0076`` keeps a running timer on the value it
/// was loaded with, so a change never moves a window already open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerParameter {
    /// `tS3_Server`.
    S3Server(u32),
    /// `tP2_Server_Max`.
    P2ServerMax(u32),
    /// `tP2*_Server_Max`.
    P2StarServerMax(u32),
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

/// What opening a physical channel supplies.
///
/// ``UDSS_LLR_0126`` holds these with the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalChannelParams {
    /// ``UDSS_LLR_0132`` — the response window pair.
    pub reloads: Reloads,
    /// ``UDSS_LLR_0165`` — `tP3_Client_Phys`.
    pub spacing: u32,
    /// ``UDSS_LLR_0152`` — `tS3_Client`, present in physical keep-alive, where
    /// ``UDSS_LLR_0151`` puts the fact and timer on each physical channel, and absent in
    /// functional keep-alive, where no requirement gives a physical channel's `tS3_Client`
    /// a meaning. Opening a channel where this disagrees with the client's keep-alive mode
    /// is rejected under [`crate::Cause::S3ClientReloadMismatch`], as ``UDSS_LLR_0152``
    /// requires.
    pub s3_client: Option<u32>,
}

/// What opening a functional channel supplies.
///
/// ``UDSS_LLR_0126`` holds these with the channel. There is no `tS3_Client`:
/// ``UDSS_LLR_0150`` keeps functional keep-alive's single timer with the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionalChannelParams {
    /// ``UDSS_LLR_0132`` — the response window pair.
    pub reloads: Reloads,
    /// ``UDSS_LLR_0165`` — `tP3_Client_Func`.
    pub spacing: u32,
}

/// One physical channel parameter, for setting it again.
///
/// ``UDSS_LLR_0043``; ``UDSS_LLR_0134`` rejects a setting naming a channel the client
/// does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicalChannelParameter {
    /// The default response reload.
    DefaultReload(u32),
    /// The enhanced response reload.
    EnhancedReload(u32),
    /// The request spacing.
    Spacing(u32),
    /// `tS3_Client`.
    S3Client(u32),
}

/// One functional channel parameter, for setting it again.
///
/// ``UDSS_LLR_0043``; ``UDSS_LLR_0134`` rejects a setting naming a channel the client
/// does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionalChannelParameter {
    /// The default response reload.
    DefaultReload(u32),
    /// The enhanced response reload.
    EnhancedReload(u32),
    /// The request spacing.
    Spacing(u32),
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelReload, FunctionalChannelParams, PhysicalChannelParams, Reloads,
        ServerParams,
    };

    /// ``UDSS_LLR_0151`` and ``UDSS_LLR_0152`` — a `tS3_Client` belongs to a physical
    /// channel in physical keep-alive and to no functional channel. The types carry
    /// that: there is no field on which to state one for a functional channel.
    #[test]
    fn only_a_physical_channel_carries_a_session_reload() {
        let reloads = Reloads {
            default_reload: 50,
            enhanced_reload: 5_000,
        };
        let physical = PhysicalChannelParams {
            reloads,
            spacing: 60,
            s3_client: Some(2_000),
        };
        let functional = FunctionalChannelParams {
            reloads,
            spacing: 70,
        };
        assert_eq!(physical.s3_client, Some(2_000));
        assert_eq!(functional.reloads.default_reload, 50);
    }

    /// ``UDSS_LLR_0119`` derives the response-pending spacing from `tP2*_Server_Max`
    /// rather than taking a parameter, so `ServerParams` has three fields and not four.
    #[test]
    fn the_server_has_no_spacing_parameter() {
        let p = ServerParams {
            s3_server: 5_000,
            p2_server_max: 50,
            p2_star_server_max: 5_000,
        };
        assert_eq!(p.p2_star_server_max, 5_000);
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
