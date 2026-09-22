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
///
/// The inverse of [`to_logical`], and the direction `classify` takes: an
/// inbound diagnostic message carries the responder's logical address, which
/// becomes the `S_AI[SA]` the driver needs to tell functional responses apart.
#[must_use]
pub const fn from_logical(addr: simple_doip::LogicalAddress) -> Address {
    Address(addr.0)
}

/// Why a connection closed.
///
/// ISO 14229-5:2022 REQ 7.9 and REQ 7.11 make a server-initiated close part of
/// the `DiagnosticSessionControl` and `ECUReset` flows, so a close is not
/// necessarily a fault. This becomes
/// `uds_services::TransportEvent::Closed`'s `expected`, which is binary because
/// the driver's decision is; see `a_close_cause_is_the_seams_expected_flag`.
// `cfg_attr(not(test), ...)` rather than a bare `expect`: the tests below
// construct both variants, so under `cfg(test)` the type is not dead and a bare
// expectation goes unfulfilled. The suppression is for the non-test build, where
// `classify` is still a `todo!()`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "constructed once classify's body replaces its todo!()"
    )
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseCause {
    /// Expected: the server closed after a positive response and before
    /// executing the service (REQ 7.9, REQ 7.11).
    ServiceInitiated,
    /// Unexpected: the transport failed.
    TransportFailure,
}

impl CloseCause {
    /// `uds_services::TransportEvent::Closed`'s `expected` flag.
    ///
    /// The seam's flag is binary because the driver's decision is — reconnect
    /// and repeat routing activation, or fail the exchange — so the distinction
    /// this enum keeps is the one this crate needs in order to *make* that
    /// decision, not one the driver would act on differently.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called once next_event's body replaces its todo!()"
        )
    )]
    pub(crate) const fn expected(self) -> bool {
        matches!(self, Self::ServiceInitiated)
    }
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
/// type cannot yet be *received*: `simple_doip`'s `Payload` models only the
/// types ISO 13400-2 defines, with no catch-all carrying an unmodelled type's
/// bytes. Delivery is no longer a gap —
/// `uds_services::TransportEvent::Periodic` exists as of 2026-09-17 — so one
/// half of this is closed and the constant still promises nothing until the
/// other is.
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
        /// Derived from the acknowledgement's **code**, never from its payload
        /// type. The two disagree in practice: `simple_doip`'s
        /// `Message::diagnostic_message_ack` stamps the positive payload type
        /// (`0x8002`) into the header whatever the ack code says — its own
        /// documented limitation — so reading the payload type would report a
        /// rejection as an acceptance and start `tP_Client` for a message the
        /// entity never accepted. ISO 13400-2 makes the code the authority
        /// regardless of which crate is emitting.
        ///
        /// A rejection cannot yet say why. `SResult::Transport` carries a
        /// `TransportError(u16)` precisely so a lower layer's own code reaches
        /// the driver unchanged, and there is nothing to put in it:
        /// `Payload::decode` maps a received `0x8003` to a fieldless variant,
        /// discarding the NACK code, both addresses and the echoed request
        /// bytes. Every rejection therefore collapses to "the transport refused
        /// it", where ISO 13400-2 distinguishes an unknown target address from
        /// routing not activated from an out-of-memory entity — three failures a
        /// tester acts on differently. Raised with `simple_doip` 2026-09-17.
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

/// Classify an inbound `DoIP` message from its header and its payload bytes.
///
/// `Ok(None)` for a payload type this crate assigns no [`DoIpEvent`] meaning —
/// one of ISO 13400-2's non-diagnostic payload types (routing activation,
/// vehicle identification, and so on), which belong to `simple_doip`'s own
/// connection handling rather than to a UDS exchange.
///
/// # Why this takes a header rather than a decoded `Message`
///
/// Because `0x8004` is not decodable by the layer below, and routing it there
/// turns a conformant message into an error.
///
/// ISO 13400-2:2019 does not define `0x8004`; ISO 14229-5:2022 REQ 7.16 adds
/// it. `simple_doip` maps every value it does not model to
/// `PayloadType::Reserved`, and `Payload::decode` returns a `MessageError` for
/// `Reserved(_)` rather than decoding it — correctly, since it cannot know what
/// the bytes mean. So a periodic response handed to `Message::decode` comes
/// back as a wire error, and a driver that tears down a connection on an `Err`
/// would do so on a message the standard requires the server to send. That is
/// the same failure `uds_services::TransportEvent::Closed` exists to prevent
/// for an expected close.
///
/// Reading [`Header::payload_type`](simple_doip::messages::Header) first keeps
/// the decision here: `Payload::decode` is called only for the types it models,
/// and `0x8004` is this crate's to interpret, which is what
/// [`PERIODIC_RESPONSE_PAYLOAD_TYPE`] has always said it was. Both facts are
/// pinned by `tests::the_layer_below_cannot_decode_a_periodic_response`, which
/// starts failing when `simple_doip` gains a catch-all.
///
/// The generic header is read before the payload in any case — it is what
/// carries the payload length, and so what decides truncation — so this costs
/// no extra read.
///
/// # Errors
///
/// A [`MessageError`](simple_doip::messages::MessageError) from the layer below
/// for a payload it models but cannot decode.
#[expect(
    unused_variables,
    reason = "header and payload are unused until classify's body replaces the todo!()"
)]
#[expect(
    dead_code,
    reason = "called once next_event's body replaces its todo!()"
)]
pub(crate) fn classify<'a>(
    header: &simple_doip::messages::Header,
    payload: &'a [u8],
) -> Result<Option<DoIpEvent<'a>>, simple_doip::messages::MessageError> {
    todo!(
        "branch on header.payload_type: 0x8001 -> Ind, 0x8002/0x8003 -> Conf, \
         0x8004 -> Periodic decoded here, everything else -> Ok(None)"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CloseCause, DoIpEvent, MappingError, PERIODIC_RESPONSE_PAYLOAD_TYPE, target_of,
    };
    use simple_doip::messages::{Payload, PayloadType};
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

    /// Every classified case has a seam event to become.
    ///
    /// This match is exhaustive and the crate denies `wildcard_enum_match_arm`,
    /// so a new [`DoIpEvent`] case breaks this build rather than being quietly
    /// absorbed by a `_` arm.
    ///
    /// It replaces `the_two_cases_with_nowhere_to_go`, which held open the two
    /// holes this seam used to have: a periodic response (REQ 7.16) and a
    /// connection close (REQ 7.9, REQ 7.11) could both be classified here and
    /// had nowhere to be delivered. `uds_services` published
    /// `TransportEvent::Periodic` and `TransportEvent::Closed` on 2026-09-17,
    /// so the holes are closed and this asserts the opposite property.
    ///
    /// The danger it exists for is unchanged: `classify` constructs all four
    /// cases, so `#[expect(dead_code)]` on [`DoIpEvent`] clears itself whether
    /// or not each case reaches the driver. Nothing else in the build would
    /// notice a periodic response being decoded and dropped.
    #[test]
    fn every_classified_case_has_a_seam_event() {
        /// The `uds_services::TransportEvent` case this becomes. A `&'static
        /// str` rather than a constructed event, because building one needs a
        /// buffer to borrow and the mapping is what is under test.
        fn seam_case(event: &DoIpEvent<'_>) -> &'static str {
            match event {
                DoIpEvent::Ind { .. } => "DataInd",
                DoIpEvent::Conf { .. } => "DataConf",
                DoIpEvent::Periodic { .. } => "Periodic",
                DoIpEvent::Closed { .. } => "Closed",
            }
        }

        assert_eq!(
            seam_case(&DoIpEvent::Ind {
                source: Address(0x0E80),
                data: &[],
            }),
            "DataInd",
        );
        assert_eq!(
            seam_case(&DoIpEvent::Closed {
                cause: CloseCause::ServiceInitiated,
            }),
            "Closed",
            "ISO 14229-5:2022 REQ 7.9 / 7.11",
        );
        assert_eq!(
            seam_case(&DoIpEvent::Periodic {
                source: Address(0x0E80),
                pdid: 0x01,
                data: &[],
            }),
            "Periodic",
            "ISO 14229-5:2022 REQ 7.16",
        );
    }

    /// Only a close the standard prescribes is reported as expected.
    ///
    /// The direction matters more than the mapping: reporting a prescribed
    /// close as unexpected makes the driver fail a conformant
    /// `DiagnosticSessionControl` or `ECUReset` flow, which is the failure
    /// `uds_services::TransportEvent::Closed` exists to prevent.
    #[test]
    fn only_a_prescribed_close_is_expected() {
        assert!(
            CloseCause::ServiceInitiated.expected(),
            "REQ 7.9 / 7.11 make this close part of the flow",
        );
        assert!(
            !CloseCause::TransportFailure.expected(),
            "a link that went away is not a flow the standard prescribes",
        );
    }

    /// The layer below cannot decode a periodic response, which is why
    /// [`classify`](super::classify) reads the payload type from the header
    /// rather than handing the message to `Payload::decode`.
    ///
    /// Two facts, both `simple_doip`'s and neither ours to assume: `0x8004` is
    /// a payload type it does not model, and an unmodelled type is an **error**
    /// from `Payload::decode` rather than something it passes through. The
    /// second is what makes this a conformance problem instead of a missing
    /// capability — ISO 14229-5:2022 REQ 7.16 requires the server to send this
    /// message, and routing it through the layer below reports it as a wire
    /// failure.
    ///
    /// **When this test starts failing**, `simple_doip` has gained the
    /// catch-all variant asked for in
    /// `2026-09-17-uds_on_ip-payload-and-ack-gaps.md`. That is good news:
    /// delete this test and let the message decode normally.
    #[test]
    fn the_layer_below_cannot_decode_a_periodic_response() {
        let periodic = PayloadType::from(PERIODIC_RESPONSE_PAYLOAD_TYPE);
        assert_eq!(
            periodic,
            PayloadType::Reserved(PERIODIC_RESPONSE_PAYLOAD_TYPE),
            "ISO 13400-2:2019 stops at 0x8003; 0x8004 is REQ 7.16's addition",
        );
        assert!(
            Payload::decode(&[], periodic).is_err(),
            "an unmodelled payload type is an error below, so a conformant \
             periodic response must never be routed through Payload::decode",
        );
    }

    /// A rejection is stamped with the *positive* payload type, so the ack code
    /// is the only thing that may be read.
    ///
    /// `Message::diagnostic_message_ack` writes
    /// `PayloadType::DiagnosticMessagePositiveAcknowledge` into the header
    /// whatever the code says — `simple_doip`'s own documented limitation. This
    /// pins it, because `DoIpEvent::Conf`'s rule depends on it being true: a
    /// transport reading the payload type would report a rejection as an
    /// acceptance and start `tP_Client` for a message the entity never
    /// accepted.
    ///
    /// **When this test starts failing**, `simple_doip` has fixed it. Delete
    /// the test; the rule it defends stays correct either way, because
    /// ISO 13400-2 makes the code authoritative regardless.
    #[test]
    fn a_rejection_is_stamped_with_the_positive_payload_type() {
        use simple_doip::messages::{DiagnosticAckCode, Message};

        let rejected = Message::diagnostic_message_ack(
            simple_doip::messages::ProtocolVersion::V2019,
            simple_doip::LogicalAddress(0x0E80),
            simple_doip::LogicalAddress(0x0E00),
            DiagnosticAckCode::UnknownTargetAddress,
            &[],
        );

        assert_eq!(
            rejected.header.payload_type,
            PayloadType::DiagnosticMessagePositiveAcknowledge,
            "the header says positive for a rejection, so only the ack code \
             may be read to derive an SResult",
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
