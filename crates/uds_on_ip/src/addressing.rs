//! Addressing information, as defined by ISO 14229-2:2021 clause 8.
//!
//! The service primitives at every layer carry the same addressing triple —
//! source address, target address, and target address type — under
//! layer-specific names (`A_SA`/`S_SA`/`T_SA`/`DoIP_SA` and so on). ISO
//! 14229-5:2022 REQ 4.4 Table 5 maps the transport-layer names onto the `DoIP`
//! ones one-for-one, so a single type serves the whole crate.

pub use simple_doip::LogicalAddress;

/// Message type, `Mtype` in ISO 14229-2:2021 clause 8.
///
/// Only [`Mtype::Diagnostics`] is reachable over `DoIP` today;
/// `RemoteDiagnostics` exists because the standard defines it and because
/// omitting it would make the mapping to `S_Mtype` lossy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mtype {
    /// Ordinary diagnostic communication.
    Diagnostics,
    /// Remote diagnostic communication.
    RemoteDiagnostics,
}

/// Target address type, `TAtype` in ISO 14229-2:2021 clause 8.
///
/// The distinction is load-bearing rather than cosmetic: it selects which
/// `tP3_Client` timer applies, and it determines whether a request is answered
/// by one server or by several. See [`crate::client`] for the consequence at
/// the API surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaType {
    /// Addressed to exactly one server. At most one response.
    Physical,
    /// Addressed to a group. Answered by every server that supports the
    /// request, so zero or more responses (ISO 14229-5:2022 Figure 8).
    Functional,
}

/// The addressing triple carried by every service primitive.
///
/// ISO 14229-2 writes this as `AI[SA]`, `AI[TA]`, `AI[TAtype]`. The address
/// extension `AI[AE]` is deliberately absent: ISO 14229-5:2022 REQ 4.4 Table 5
/// records `T_AE` as not applicable, because `DoIP` does not support address
/// extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ai {
    /// Source address — this node.
    pub sa: LogicalAddress,
    /// Target address — the peer, or the group.
    pub ta: LogicalAddress,
    /// Whether `ta` names one server or a group.
    pub ta_type: TaType,
    /// Message type.
    pub mtype: Mtype,
}

impl Ai {
    /// A physically addressed diagnostic channel between two entities.
    #[must_use]
    pub const fn physical(sa: LogicalAddress, ta: LogicalAddress) -> Self {
        Self { sa, ta, ta_type: TaType::Physical, mtype: Mtype::Diagnostics }
    }

    /// A functionally addressed diagnostic channel to a group address.
    #[must_use]
    pub const fn functional(sa: LogicalAddress, ta: LogicalAddress) -> Self {
        Self { sa, ta, ta_type: TaType::Functional, mtype: Mtype::Diagnostics }
    }
}

/// Identifies one logical communication channel.
///
/// ISO 14229-2:2021 REQ 5.26 Table 7 requires a separate `tP_Client` timer for
/// each logical communication channel, physical and functional alike, so the
/// session layer needs a way to name them. A `ChannelId` is opaque and is
/// issued by [`crate::session::SessionLayer::open_channel`]; it is meaningless
/// outside the session instance that issued it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChannelId(pub(crate) u16);

impl ChannelId {
    /// The raw index, for callers that need to key their own storage by channel.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}
