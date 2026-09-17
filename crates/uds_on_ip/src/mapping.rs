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

use uds_session::{Address, Ai, Mtype, SResult};

/// A constraint of the `DoIP` mapping, not of the session layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MappingError {
    /// ISO 14229-5:2022 REQ 4.4 Table 5 records `T_AE` as not applicable to
    /// `DoIP`, so `Mtype::RDiag` and `Mtype::SecureRDiag` cannot be carried.
    ///
    /// Expressible in `uds_session` and rejected here is the correct
    /// arrangement: the session layer keeps all four of the standard's
    /// message types, and the transport that cannot carry two of them says so.
    #[error("DoIP has no address extension (ISO 14229-5 REQ 4.4 Table 5)")]
    AddressExtensionUnsupported,
}

/// ISO 14229-2 `S_TA` to ISO 13400-2 logical address.
#[must_use]
pub const fn to_logical(addr: Address) -> simple_doip::LogicalAddress {
    simple_doip::LogicalAddress(addr.0)
}

/// ISO 13400-2 logical address to ISO 14229-2 `S_TA`.
#[must_use]
pub const fn from_logical(addr: simple_doip::LogicalAddress) -> Address {
    Address(addr.0)
}

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

/// The `DoIP` payload type carrying UDS periodic responses.
///
/// Introduced by ISO 14229-5:2022 REQ 7.16, **not** by ISO 13400-2:2019, whose
/// diagnostic payload types stop at `0x8003`. It is therefore this crate's to
/// interpret rather than `simple_doip`'s to name — see `ARCHITECTURE.md` §3.1.
pub const PERIODIC_RESPONSE_PAYLOAD_TYPE: u16 = 0x8004;

/// The `DoIP` target address for this addressing triple, or why it has none.
///
/// # Errors
///
/// [`MappingError::AddressExtensionUnsupported`] for the two remote message
/// types.
pub fn target_of(ai: Ai) -> Result<simple_doip::LogicalAddress, MappingError> {
    match ai.mtype {
        Mtype::Diag | Mtype::SecureDiag => Ok(to_logical(ai.ta)),
        Mtype::RDiag { .. } | Mtype::SecureRDiag { .. } => {
            Err(MappingError::AddressExtensionUnsupported)
        }
    }
}

/// An inbound `DoIP` message, classified.
///
/// Crate-internal in effect: `transport` translates the two that cross the
/// stack's seam into the driver's event type and handles the other two itself.
#[derive(Debug)]
pub enum DoIpEvent<'a> {
    /// `T_Data.ind` — a diagnostic message (`DoIP` `0x8001`).
    Ind {
        /// The responding entity. Under functional addressing this differs
        /// between responses, and is the only way to tell them apart.
        source: Address,
        /// The UDS payload, opaque at this layer.
        data: &'a [u8],
    },
    /// `T_Data.conf` — derived from a diagnostic message acknowledgement
    /// (`DoIP` `0x8002`/`0x8003`), **never** from a completed socket write,
    /// because the acknowledgement is what starts `tP_Client`
    /// (ISO 14229-2:2021 REQ 5.9).
    Conf {
        /// The acknowledging entity.
        peer: Address,
        /// `SResult::Ok` for `0x8002`; a `0x8003` becomes one
        /// `SResult::Transport` value carrying the NACK code.
        result: SResult,
    },
    /// A periodic response (`DoIP` `0x8004`).
    ///
    /// Deliberately not a `T_Data.ind`: ISO 14229-5:2022 REQ 7.20 requires
    /// that unsolicited responses do not reset `tS3_Server`, so these bypass
    /// the request/response path entirely.
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
        /// Whether the close was expected.
        cause: CloseCause,
    },
}

/// Classify an inbound `DoIP` message.
///
/// `None` for a payload type this crate assigns no [`DoIpEvent`] meaning —
/// one of ISO 13400-2's non-diagnostic payload types (routing activation,
/// vehicle identification, and so on), which belong to `simple_doip`'s own
/// connection handling rather than to a UDS exchange.
///
/// # Prototype gap — `0x8004` is currently unrepresentable
///
/// `simple_doip`'s `Payload` models exactly the payload types ISO 13400-2
/// defines, with no catch-all carrying an unmodelled type's bytes, so
/// [`DoIpEvent::Periodic`] cannot be constructed. This needs no UDS semantics
/// in `simple_doip` — only a variant meaning "a payload type I do not model,
/// and here are its bytes". Named in that repository's brief §5.
#[expect(
    unused_variables,
    reason = "message is unused until classify's body replaces the todo!() above"
)]
#[must_use]
pub fn classify<'a>(message: &simple_doip::messages::Message<'a>) -> Option<DoIpEvent<'a>> {
    todo!("classify Payload into a DoIpEvent; blocked on the 0x8004 gap above")
}

#[cfg(test)]
mod tests {
    use super::{target_of, MappingError};
    use uds_session::{Address, AddressExtension, Ai, Mtype, TaType};

    fn ai_with(mtype: Mtype) -> Ai {
        Ai {
            mtype,
            sa: Address(0x0E00),
            ta: Address(0x0E80),
            ta_type: TaType::Physical,
        }
    }

    /// ISO 14229-5:2022 REQ 4.4 Table 5 marks `T_AE` not applicable to `DoIP`.
    /// The constraint lands here, where it is true, rather than deforming
    /// `uds_session::Mtype` into two variants.
    #[test]
    fn a_remote_message_type_is_rejected() {
        let ae = AddressExtension(0x0001);
        assert_eq!(
            target_of(ai_with(Mtype::RDiag { ae })),
            Err(MappingError::AddressExtensionUnsupported)
        );
        assert_eq!(
            target_of(ai_with(Mtype::SecureRDiag { ae })),
            Err(MappingError::AddressExtensionUnsupported)
        );
    }

    #[test]
    fn a_local_message_type_maps_to_its_target() {
        assert_eq!(
            target_of(ai_with(Mtype::Diag)).map(|a| a.0),
            Ok(0x0E80)
        );
        assert_eq!(
            target_of(ai_with(Mtype::SecureDiag)).map(|a| a.0),
            Ok(0x0E80)
        );
    }
}
