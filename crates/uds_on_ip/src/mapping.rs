//! Transport layer: the `T_PDU` ↔ `DoIP_PDU` mapping.
//!
//! ISO 14229-5:2022 clause 11. REQ 4.3 Table 4 maps the service primitives
//! one-for-one (`T_Data.request` ↔ `DoIP_Data.request`, and so on), and REQ 4.4
//! Table 5 maps the parameters (`T_SA` ↔ `DoIP_SA`, `T_TA` ↔ `DoIP_TA`, …) with
//! `T_AE` marked not applicable because `DoIP` has no address extension.
//!
//! The mapping is nearly an identity, so this module is thin. Its one piece of
//! real work is deciding *which* transport event an inbound `DoIP` message is —
//! and in particular recognising that a diagnostic message acknowledgement is a
//! `T_Data.conf`, because that is what starts `tP_Client`
//! (ISO 14229-2:2021 REQ 5.9).

use crate::addressing::{Address, ChannelId};
use crate::session::TResult;

/// Why a connection closed.
///
/// ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a server-initiated close part of
/// the `DiagnosticSessionControl` and `ECUReset` flows, so a close is not
/// necessarily a fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseCause {
    /// Expected: the server closed after a positive response and before
    /// executing the service (REQ 7.9, REQ 7.11).
    ServiceInitiated,
    /// Unexpected: the transport failed.
    TransportFailure,
}

/// An inbound transport-layer event, ready to be offered to the session layer.
#[derive(Debug)]
pub enum TransportEvent<'a> {
    /// `T_Data.conf` — derived from a diagnostic message acknowledgement
    /// (`DoIP` `0x8002`/`0x8003`), never from a completed socket write.
    Conf {
        /// The channel the confirmed request belonged to.
        channel: ChannelId,
        /// Whether the peer accepted the message.
        result: TResult,
    },
    /// `T_Data.ind` — a diagnostic message (`DoIP` `0x8001`).
    Ind {
        /// The channel it arrived on.
        channel: ChannelId,
        /// The responding entity. For a functionally addressed request this
        /// differs between responses.
        source: Address,
        /// The UDS payload, opaque at this layer.
        data: &'a [u8],
    },
    /// A periodic response (`DoIP` `0x8004`).
    ///
    /// Deliberately *not* a `T_Data.ind`: ISO 14229-5:2022 REQ 7.20 requires
    /// that unsolicited responses do not reset `tS3_Server`, so these bypass
    /// the session layer's request/response path entirely.
    Periodic {
        /// The responding entity.
        source: Address,
        /// The periodic data identifier.
        pdid: u8,
        /// The periodic data record.
        data: &'a [u8],
    },
    /// The connection closed.
    Closed {
        /// The affected channel.
        channel: ChannelId,
        /// Whether the close was expected.
        cause: CloseCause,
    },
}

/// The `DoIP` payload type carrying UDS periodic responses.
///
/// Introduced by ISO 14229-5:2022 REQ 7.16, **not** by ISO 13400-2:2019, whose
/// diagnostic payload types stop at `0x8003`. It is therefore this crate's to
/// interpret rather than `simple_doip`'s to name — see `ARCHITECTURE.md` §3.1.
pub const PERIODIC_RESPONSE_PAYLOAD_TYPE: u16 = 0x8004;

/// Classify an inbound `DoIP` message as a transport-layer event.
///
/// # Prototype gap — `0x8004` is currently unrepresentable
///
/// `simple_doip`'s `Payload` and `OwnedPayload` both model exactly the payload
/// types ISO 13400-2 defines, with no catch-all variant carrying an unmodelled
/// type's bytes. A `0x8004` message therefore cannot be delivered through
/// either API, and [`TransportEvent::Periodic`] cannot be constructed.
///
/// This does not need `simple_doip` to learn any UDS semantics: it needs only a
/// variant meaning "a payload type I do not model, and here are its bytes".
/// Recorded here so the requirement is visible at the point it bites.
///
/// # Borrowed, not owned
///
/// This takes `simple_doip`'s borrowed `Message<'a>` rather than its
/// `OwnedMessage`, so the returned event can borrow the receive buffer and this
/// module stays alloc-free. `OwnedMessage` is `alloc`-gated upstream, which is
/// the correct place for that line to fall: owning a frame is a driver
/// concern, not a mapping one.
#[allow(unused_variables)]
#[must_use]
pub fn classify<'a>(
    message: &simple_doip::messages::Message<'a>,
) -> Option<TransportEvent<'a>> {
    todo!("classify Payload into a TransportEvent; blocked on the 0x8004 gap above")
}
