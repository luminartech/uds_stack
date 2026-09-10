//! The service primitives, derived from the standard rather than invented.
//!
//! ISO 14229 specifies every layer boundary as a triad of primitives —
//! `.req` to send, `.ind` to deliver something received, `.conf` to report
//! that a send completed. ISO 14229-2:2021 clause 7 Table 1 specifies exactly
//! which parameters are valid on each, and that table is the reason these are
//! three separate types rather than one type with optional fields:
//!
//! | Parameter | `.req` | `.ind` | `.conf` |
//! | --- | --- | --- | --- |
//! | `Mtype`, `AI[TAtype]`, `AI[TA]`, `AI[SA]`, `AI[AE]` | ✓ | ✓ | ✓ |
//! | `Length`, `Data` | ✓ | ✓ | — |
//! | `Result` | — | ✓ | ✓ |
//!
//! A confirmation carries no data; a request carries no result. Modelling all
//! three as one struct would make both of those representable.
//!
//! # `Length` is not a field
//!
//! The standard lists `S_Length` beside `S_Data` because a service interface
//! expressed in C must pass a length alongside a pointer. A Rust slice carries
//! its own length, so `S_Length` is `data.len()` and a separate field could
//! only ever disagree with it. This is the one place these types deliberately
//! depart from the parameter list.

use crate::addressing::Ai;

/// `S_Result` — the outcome of a service execution
/// (ISO 14229-2:2021 clause 8.10).
///
/// The standard specifies the range as `[OK, ERR_...]`, deliberately leaving
/// the error set open: clause 8.10 says the error is issued "when an error is
/// detected by a lower layer (provider)", so which errors exist is a property
/// of the transport underneath. The variants below are the ones a `DoIP`
/// provider can report; the type is `#[non_exhaustive]` because that set
/// belongs to the transport rather than to this standard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SResult {
    /// Service execution completed successfully.
    ///
    /// Clause 8.10 is explicit that `OK` is issued to the service user on both
    /// the sender and the receiver side, which is why it appears on both
    /// [`Indication`] and [`Confirm`].
    Ok,
    /// `tP_Client` expired without a complete response
    /// (ISO 14229-2:2021 REQ 5.16).
    ///
    /// On a functionally addressed channel this is not a failure: expiry is
    /// the defined signal that no further responses are coming
    /// (ISO 14229-5:2022 Figure 8).
    Timeout,
    /// The peer refused the message at the `DoIP` layer — a diagnostic message
    /// negative acknowledgement, payload type `0x8003`.
    Rejected,
    /// The transport failed before the exchange completed.
    TransportError,
}

impl SResult {
    /// Whether this outcome is `OK`.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// `A_Data.req` / `S_Data.req` — request transmission
/// (ISO 14229-2:2021 clause 7.4).
///
/// Carries address information and data, and no result: the outcome arrives
/// later as a [`Confirm`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request<'a> {
    /// `S_Mtype` and `S_AI[..]`.
    pub ai: Ai,
    /// `S_Data`. `S_Length` is its length.
    pub data: &'a [u8],
}

/// `A_Data.ind` / `S_Data.ind` — deliver a received message
/// (ISO 14229-2:2021 clause 7.5).
///
/// Carries a result as well as data. That is not redundant: clause 7.5 says
/// the primitive "shall indicate `S_Result` events **and** deliver `S_Data`",
/// so an indication can report a failure detected on the receive path rather
/// than only a successful delivery. Check [`Indication::result`] before
/// trusting [`Indication::data`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Indication<'a> {
    /// `S_Mtype` and `S_AI[..]`, as seen from the receiver.
    ///
    /// For a functionally addressed request this is how responses are told
    /// apart: each responding server sets its own `S_AI[SA]`.
    pub ai: Ai,
    /// `S_Data`. Meaningful only when `result` is
    /// [`SResult::Ok`].
    pub data: &'a [u8],
    /// `S_Result`.
    pub result: SResult,
}

/// `A_Data.conf` / `S_Data.conf` — confirm that a request completed
/// (ISO 14229-2:2021 clause 7.6).
///
/// Carries no data, by Table 1. On `DoIP` this is raised when the diagnostic
/// message acknowledgement arrives, not when the socket write returns, because
/// it is what starts `tP_Client` (ISO 14229-2:2021 REQ 5.9).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Confirm {
    /// `S_Mtype` and `S_AI[..]`.
    pub ai: Ai,
    /// `S_Result`.
    pub result: SResult,
}

/// How a request/response exchange ended.
///
/// # Why a confirmation can be the whole answer
///
/// A request whose `suppressPosRspMsgIndicationBit` is set is answered with
/// **no response at all** when it succeeds, and so is a functionally addressed
/// request that the server does not support (ISO 14229-1:2020 clause 8.7.3.3
/// and 8.7.4.3, where `SNS`, `SNSIAS`, `SFNS`, `SFNSIAS` and `ROOR` produce
/// silence rather than a negative response).
///
/// For those exchanges the confirmation *is* the completion, and an API shaped
/// as "send, then return the response" has no way to say so — it can only
/// invent a timeout for a request that behaved exactly as specified. Hence two
/// variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion<'a> {
    /// A response arrived.
    Responded(Indication<'a>),
    /// The request was transmitted and no response is expected.
    Confirmed(Confirm),
}

impl<'a> Completion<'a> {
    /// The response bytes, if a response arrived and it was `OK`.
    #[must_use]
    pub const fn data(&self) -> Option<&'a [u8]> {
        match self {
            Self::Responded(ind) if ind.result.is_ok() => Some(ind.data),
            _ => None,
        }
    }

    /// The `S_Result` of whichever primitive completed the exchange.
    #[must_use]
    pub const fn result(&self) -> SResult {
        match self {
            Self::Responded(ind) => ind.result,
            Self::Confirmed(conf) => conf.result,
        }
    }
}
