//! Addressing information, as defined by ISO 14229-2:2021 clause 8.
//!
//! Every layer's service primitives carry the same address information under
//! layer-specific names (`A_SA`/`S_SA`/`T_SA`/`DoIP_SA` and so on). ISO
//! 14229-2:2021 clause 7 Table 1 maps the application-layer names onto the
//! session-layer ones one-for-one, and ISO 14229-5:2022 REQ 4.4 Table 5 maps
//! the transport-layer names onto the `DoIP` ones, so a single set of types
//! serves the whole crate.

/// A session-layer address: `S_TA` (clause 8.5) or `S_SA` (clause 8.6).
///
/// Both are specified as data type Unsigned Word over the full range
/// `0x0000`–`0xFFFF`, so no value is reserved or invalid at this layer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Address(pub u16);

impl From<Address> for simple_doip::LogicalAddress {
    fn from(a: Address) -> Self {
        Self(a.0)
    }
}

impl From<simple_doip::LogicalAddress> for Address {
    fn from(a: simple_doip::LogicalAddress) -> Self {
        Self(a.0)
    }
}

/// `S_Mtype`, the session layer message type (ISO 14229-2:2021 clause 8.3).
///
/// `S_Mtype` is not decoration: it *determines which address information
/// parameters apply*. Clause 8.3 specifies four values in two groups —
/// `Diag`/`SecureDiag` carry `{S_SA, S_TA, S_TAtype}`, while
/// `RDiag`/`SecureRDiag` additionally carry `S_AE`.
///
/// # Why only two of the four appear here
///
/// The remote-diagnostics message types require an address extension, and ISO
/// 14229-5:2022 REQ 4.4 Table 5 records `T_AE` as **not applicable** because
/// `DoIP` does not support address extension. They are therefore not
/// expressible over this transport, and modelling them would mean offering a
/// variant that can never be sent.
///
/// This is why [`Ai`] has no `ae` field: rather than an `Option` that is always
/// `None`, the type system carries the constraint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mtype {
    /// `Diag` — ordinary diagnostic communication.
    #[default]
    Diag,
    /// `SecureDiag` — secured diagnostic communication.
    SecureDiag,
}

/// `S_TAtype`, the session layer target address type
/// (ISO 14229-2:2021 clause 8.4).
///
/// Clause 8.4 describes this as a configuration attribute of `S_TA` encoding
/// the communication model between the peers. The distinction is load-bearing
/// rather than descriptive: it selects which `tP3_Client` timer applies, it
/// decides whether a request is answered by one server or several, and under
/// ISO 14229-1 clause 8.7 it decides whether an unsupported request is
/// answered with a negative response or with silence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaType {
    /// One peer. At most one response.
    Physical,
    /// A group. Answered by every server that supports the request, so zero or
    /// more responses (ISO 14229-5:2022 Figure 8).
    Functional,
}

/// Address information — `S_AI` in ISO 14229-2:2021 clause 7.
///
/// The address extension `S_AI[AE]` is absent by construction; see [`Mtype`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ai {
    /// `S_Mtype` (clause 8.3).
    pub mtype: Mtype,
    /// `S_AI[SA]` — the sending entity (clause 8.6).
    pub sa: Address,
    /// `S_AI[TA]` — the receiving entity, or the group (clause 8.5).
    pub ta: Address,
    /// `S_AI[TAtype]` (clause 8.4).
    pub ta_type: TaType,
}

impl Ai {
    /// Ordinary diagnostics addressed to one peer.
    #[must_use]
    pub const fn physical(sa: Address, ta: Address) -> Self {
        Self { mtype: Mtype::Diag, sa, ta, ta_type: TaType::Physical }
    }

    /// Ordinary diagnostics addressed to a group.
    #[must_use]
    pub const fn functional(sa: Address, ta: Address) -> Self {
        Self { mtype: Mtype::Diag, sa, ta, ta_type: TaType::Functional }
    }

    /// The same triple with the source and target exchanged, as it appears
    /// from the peer's side.
    #[must_use]
    pub const fn reversed(self) -> Self {
        Self { mtype: self.mtype, sa: self.ta, ta: self.sa, ta_type: self.ta_type }
    }
}

/// Identifies one logical communication channel.
///
/// ISO 14229-2:2021 REQ 5.26 Table 7 requires a separate `tP_Client` timer for
/// each logical communication channel, physical and functional alike, so the
/// session layer needs a way to name them. A `ChannelId` is opaque, is issued
/// by [`crate::session::SessionLayer::open_channel`], and is meaningless
/// outside the session instance that issued it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChannelId(pub(crate) u16);

impl ChannelId {
    /// The raw index, for callers keying their own storage by channel.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}
