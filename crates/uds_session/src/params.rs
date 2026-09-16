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

/// What supplying a channel's storage supplies alongside it.
///
/// ``UDSS_LLR_0126`` holds these with the channel; ``UDSS_LLR_0132``, ``UDSS_LLR_0152``
/// and ``UDSS_LLR_0165`` define them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelParams {
    /// ``UDSS_LLR_0132`` — `tP2_Client_Max`, or `tP6_Client_Max` where the transport has
    /// no `T_DataSOM.ind`. The session layer does not distinguish the two cases.
    pub default_reload: u32,
    /// ``UDSS_LLR_0132`` — `tP2*_Client_Max` or `tP6*_Client_Max`.
    pub enhanced_reload: u32,
    /// ``UDSS_LLR_0165`` — `tP3_Client_Phys` on a physical channel, `tP3_Client_Func` on
    /// a functional one.
    pub spacing: u32,
    /// ``UDSS_LLR_0152`` — `tS3_Client`, present only in physical keep-alive, where each
    /// physical channel has its own. In functional keep-alive the client has a single
    /// one, supplied with the keep-alive mode at creation.
    pub s3_client: Option<u32>,
}

/// One channel parameter, for setting it again.
///
/// ``UDSS_LLR_0043``; ``UDSS_LLR_0134`` rejects a setting naming a channel the client
/// does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelParameter {
    /// The default response reload.
    DefaultReload(u32),
    /// The enhanced response reload.
    EnhancedReload(u32),
    /// The request spacing.
    Spacing(u32),
    /// `tS3_Client`, in physical keep-alive.
    S3Client(u32),
}

#[cfg(test)]
mod tests {
    use super::{ChannelParams, ServerParams};

    /// ``UDSS_LLR_0152`` — functional keep-alive has one `tS3_Client` for the client and
    /// physical keep-alive one per physical channel, so a channel's is optional.
    #[test]
    fn a_channel_carries_a_session_reload_only_in_physical_keep_alive() {
        let functional = ChannelParams {
            default_reload: 50,
            enhanced_reload: 5_000,
            spacing: 60,
            s3_client: None,
        };
        assert_eq!(functional.s3_client, None);

        let physical = ChannelParams {
            s3_client: Some(2_000),
            ..functional
        };
        assert_eq!(physical.s3_client, Some(2_000));
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
}
