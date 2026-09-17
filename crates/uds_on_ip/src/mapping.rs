//! Transport layer: the `T_PDU` ↔ `DoIP_PDU` mapping.
//!
//! ISO 14229-5:2022 clause 11. REQ 4.3 Table 4 maps the service primitives
//! one-for-one (`T_Data.request` ↔ `DoIP_Data.request`, and so on), and REQ 4.4
//! Table 5 maps the parameters (`T_SA` ↔ `DoIP_SA`, `T_TA` ↔ `DoIP_TA`, …) with
//! `T_AE` marked not applicable because `DoIP` has no address extension.
//!
//! The mapping is nearly an identity, so this module is thin, and what it
//! publishes is thinner still: the address conversions and the one constant
//! ISO 14229-5 adds to ISO 13400-2's payload types.
//!
//! Classifying an inbound message — in particular recognising that a
//! diagnostic message acknowledgement is a `T_Data.conf`, because that is what
//! starts `tP_Client` (ISO 14229-2:2021 REQ 5.9) — happens here too, but
//! privately. It produces vocabulary the driver never sees, and publishing it
//! would leave a caller two event types and only prose to say which was
//! theirs.

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
// Constructed once `classify`'s body lands. Note that this suppression
// clearing is NOT evidence that a close reaches the driver: `classify` will
// build this, and `next_event` may then have nowhere to put it. That hole is
// held by `the_two_cases_with_nowhere_to_go` below, not by this attribute.
#[expect(
    dead_code,
    reason = "constructed once classify's body replaces its todo!()"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseCause {
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
///
/// # Nothing in this crate acts on it yet
///
/// Published as vocabulary, not as a capability. A message with this payload
/// type can be neither received (`simple_doip`'s `Payload` models only the
/// types ISO 13400-2 defines, with no catch-all) nor delivered (the event seam
/// has no periodic case). Both are open with the neighbouring crates; until
/// they close, this constant names the number and promises nothing else.
///
/// REQ 7.17's length bound — a periodic data record must not exceed the
/// non-segmented `UDSonIP` message limit — has no home here yet either. It was
/// briefly a free `periodic_record_within_limit(len) -> bool` in `profile`,
/// which no caller was obliged to consult and no path could reach; it is
/// deleted until there is a periodic record to bound, at which point the check
/// belongs where the record is accepted rather than beside it.
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
/// Crate-internal: `transport` translates the cases that cross the stack's
/// seam into the driver's event type. Not public, because a caller reading
/// this crate would otherwise face two event vocabularies with nothing but
/// prose to say which is theirs.
#[expect(
    dead_code,
    reason = "constructed once classify's body replaces its todo!()"
)]
#[derive(Debug)]
pub(crate) enum DoIpEvent<'a> {
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
        /// The acknowledgement's outcome.
        ///
        /// The rule for deriving it — read the ack code, never the payload
        /// type — is stated once, on
        /// [`TransportEvent::DataConf`](crate::TransportEvent), which is where
        /// a caller reads it. Restating it here is how the two copies drifted
        /// the first time.
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
/// and here are its bytes". Raised with that repository 2026-09-17 in
/// `2026-09-17-uds_on_ip-payload-and-ack-gaps.md`.
#[expect(
    unused_variables,
    reason = "message is unused until classify's body replaces the todo!() above"
)]
#[expect(
    dead_code,
    reason = "called once next_event's body replaces its todo!()"
)]
#[must_use]
pub(crate) fn classify<'a>(message: &simple_doip::messages::Message<'a>) -> Option<DoIpEvent<'a>> {
    todo!("classify Payload into a DoIpEvent; blocked on the 0x8004 gap above")
}

#[cfg(test)]
mod tests {
    use super::{CloseCause, DoIpEvent, MappingError, target_of};
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

    /// The two classified cases that have no seam event to become.
    ///
    /// This match is exhaustive and the crate denies `wildcard_enum_match_arm`,
    /// so a new [`DoIpEvent`] case breaks this build rather than being quietly
    /// absorbed by a `_` arm. `Periodic` and `Closed` reach `unreachable` arms
    /// *by design*: they are the hole, located here in code rather than left in
    /// a doc comment two files away.
    ///
    /// **When `TransportEvent` gains a `Periodic` or `Closed` case**, delete the
    /// corresponding arm here and translate it in `transport::next_event`. The
    /// companion guard is `transport::tests::the_seam_carries_four_cases`,
    /// which stops compiling at the same moment.
    ///
    /// The danger this exists for is specific: `classify` will construct all
    /// four cases, so `#[expect(dead_code)]` on [`DoIpEvent`] clears itself
    /// whether or not the two holes were ever filled. Nothing else in the build
    /// would notice a periodic response being decoded and dropped.
    #[test]
    fn the_two_cases_with_nowhere_to_go() {
        /// The `TransportEvent` case this would need, or `None` if it already
        /// has one. Each hole names its own missing case rather than sharing a
        /// `false` with the other, so filling one is an edit to one arm.
        fn missing_seam_case(event: &DoIpEvent<'_>) -> Option<&'static str> {
            match event {
                DoIpEvent::Ind { .. } | DoIpEvent::Conf { .. } => None,
                DoIpEvent::Periodic { .. } => Some("Periodic"),
                DoIpEvent::Closed { .. } => Some("Closed"),
            }
        }

        assert_eq!(
            missing_seam_case(&DoIpEvent::Ind {
                source: Address(0x0E80),
                data: &[],
            }),
            None,
        );
        assert_eq!(
            missing_seam_case(&DoIpEvent::Closed {
                cause: CloseCause::ServiceInitiated,
            }),
            Some("Closed"),
            "ISO 14229-5:2022 REQ 7.9 / 7.11 — a close still has no seam case",
        );
        assert_eq!(
            missing_seam_case(&DoIpEvent::Periodic {
                source: Address(0x0E80),
                pdid: 0x01,
                data: &[],
            }),
            Some("Periodic"),
            "ISO 14229-5:2022 REQ 7.16 — a periodic response still has no seam case",
        );
    }

    #[test]
    fn a_local_message_type_maps_to_its_target() {
        assert_eq!(target_of(ai_with(Mtype::Diag)).map(|a| a.0), Ok(0x0E80));
        assert_eq!(
            target_of(ai_with(Mtype::SecureDiag)).map(|a| a.0),
            Ok(0x0E80)
        );
    }
}
