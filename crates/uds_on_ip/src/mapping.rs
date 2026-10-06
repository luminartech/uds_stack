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
//! Classifying what a `DoIP` entity reports onto the transport seam happens here
//! too, but privately. It produces vocabulary the driver never sees, and
//! publishing it would leave a caller two event types and only prose to say which
//! was theirs.

use core::ops::Range;

use simple_doip::service::{ConnectionId, DoIpResult, EntityEvent};
use uds_session::{Address, Ai, Mtype, SResult, TaType, TransportError};

/// A constraint of the `DoIP` mapping, not of the session layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
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

/// The `DoIP` payload type carrying UDS periodic responses.
///
/// Introduced by ISO 14229-5:2022 REQ 7.16, **not** by ISO 13400-2:2019, whose
/// diagnostic payload types stop at `0x8003`. It is therefore this crate's to
/// interpret rather than `simple_doip`'s to name — see `ARCHITECTURE.md` §3.1.
///
/// # Nothing in this crate sends or decodes it
///
/// Published as vocabulary, not as a capability. A server sends periodic
/// responses with it (REQ 7.7, REQ 7.16), but neither
/// `uds_services::UdsTransport::t_data_req` nor
/// [`DiagnosticEntity::request`](simple_doip::service::DiagnosticEntity::request)
/// can choose a payload type, so a server built on this crate cannot. One that
/// arrives is reported by `simple_doip` as an [`EntityEvent::Unmodelled`] (an
/// [`EntityEvent::UnmodelledTruncated`] if it is too long for the buffer), and this
/// crate ignores it.
///
/// REQ 7.17's length bound — a periodic data record must not exceed the
/// non-segmented `UDSonIP` message limit — has no home here either. It belongs
/// where a periodic record is accepted, and none is. See `ARCHITECTURE.md` §9.2.
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

/// ISO 14229-2 `T_TAtype` to ISO 13400-2 `DoIP_TAtype` (ISO 14229-5:2022 REQ 4.4
/// Table 5).
pub(crate) const fn to_doip_ta_type(ta_type: TaType) -> simple_doip::TaType {
    match ta_type {
        TaType::Physical => simple_doip::TaType::Physical,
        TaType::Functional => simple_doip::TaType::Functional,
    }
}

/// ISO 13400-2 `DoIP_TAtype` to ISO 14229-2 `T_TAtype`; the inverse of
/// [`to_doip_ta_type`].
pub(crate) const fn from_doip_ta_type(ta_type: simple_doip::TaType) -> TaType {
    match ta_type {
        simple_doip::TaType::Physical => TaType::Physical,
        simple_doip::TaType::Functional => TaType::Functional,
    }
}

/// `DoIP_Result` to `T_Result` (ISO 14229-5:2022 REQ 4.4 Table 5).
///
/// ISO 13400-2:2019 8.2.5 gives `DoIP_Result` no numeric values, only a normative
/// order, so an error's [`TransportError`] is its position in that order: `DoIP_OK`
/// is 0 and becomes [`SResult::Ok`], `DoIP_ERROR` is 11.
pub(crate) const fn s_result(result: DoIpResult) -> SResult {
    let position = match result {
        DoIpResult::Ok => return SResult::Ok,
        DoIpResult::HdrError => 1,
        DoIpResult::TimeoutA => 2,
        DoIpResult::UnknownSa => 3,
        DoIpResult::InvalidSa => 4,
        DoIpResult::UnknownTa => 5,
        DoIpResult::MessageTooLarge => 6,
        DoIpResult::OutOfMemory => 7,
        DoIpResult::TargetUnreachable => 8,
        DoIpResult::NoLink => 9,
        DoIpResult::NoSocket => 10,
        DoIpResult::Error => 11,
    };
    SResult::Transport(TransportError(position))
}

/// The addressing of a `DoIP_Data` primitive as ISO 14229-2's `S_AI`.
///
/// Always [`Mtype::Diag`]: ISO 14229-5:2022 REQ 4.4 Table 5 maps `T_Ptype` onto
/// nothing, so `DoIP` carries no message type to read.
const fn ai(
    sa: simple_doip::LogicalAddress,
    ta: simple_doip::LogicalAddress,
    ta_type: simple_doip::TaType,
) -> Ai {
    Ai {
        mtype: Mtype::Diag,
        sa: from_logical(sa),
        ta: from_logical(ta),
        ta_type: from_doip_ta_type(ta_type),
    }
}

/// An [`EntityEvent`] as the transport seam needs it, with a PDU held as its place in
/// the caller's buffer so that the event no longer borrows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Inbound {
    /// `T_Data.ind` (REQ 4.3 Table 4: `DoIP_Data.indication`).
    Ind {
        connection: ConnectionId,
        ai: Ai,
        at: Range<usize>,
    },
    /// `T_Data.ind` for a message longer than the caller's buffer.
    TooLong {
        connection: ConnectionId,
        ai: Ai,
        at: Range<usize>,
        declared: usize,
    },
    /// `T_Data.conf` (REQ 4.3 Table 4: `DoIP_Data.confirm`).
    Conf {
        ai: Ai,
        result: SResult,
    },
    Closed {
        connection: ConnectionId,
    },
    Deadline,
}

/// An entity reported a PDU outside the buffer it was lent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PduOutsideBuffer;

/// Classify `event`, which borrows the buffer starting at address `buffer_start`.
///
/// `None` for an [`EntityEvent::Unmodelled`] or [`EntityEvent::UnmodelledTruncated`]: a
/// payload type ISO 14229-5 gives a server no use for — a periodic response
/// ([`PERIODIC_RESPONSE_PAYLOAD_TYPE`]) among them.
pub(crate) fn classify(
    event: EntityEvent<'_>,
    buffer_start: usize,
) -> Result<Option<Inbound>, PduOutsideBuffer> {
    let span = |pdu: &[u8]| {
        let start = pdu.as_ptr().addr().checked_sub(buffer_start)?;
        Some(start..start.checked_add(pdu.len())?)
    };
    Ok(Some(match event {
        EntityEvent::Indication {
            connection,
            sa,
            ta,
            ta_type,
            pdu,
        } => Inbound::Ind {
            connection,
            ai: ai(sa, ta, ta_type),
            at: span(pdu).ok_or(PduOutsideBuffer)?,
        },
        EntityEvent::IndicationTruncated {
            connection,
            sa,
            ta,
            ta_type,
            pdu,
            length,
        } => Inbound::TooLong {
            connection,
            ai: ai(sa, ta, ta_type),
            at: span(pdu).ok_or(PduOutsideBuffer)?,
            declared: length,
        },
        EntityEvent::Confirm {
            sa,
            ta,
            ta_type,
            result,
        } => Inbound::Conf {
            ai: ai(sa, ta, ta_type),
            result: s_result(result),
        },
        EntityEvent::Closed { connection } => Inbound::Closed { connection },
        EntityEvent::Deadline => Inbound::Deadline,
        EntityEvent::Unmodelled { .. } | EntityEvent::UnmodelledTruncated { .. } => {
            return Ok(None);
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        Inbound, MappingError, PduOutsideBuffer, classify, from_doip_ta_type, s_result,
        target_of, to_doip_ta_type,
    };
    use simple_doip::LogicalAddress;
    use simple_doip::service::{ConnectionId, DoIpResult, EntityEvent};
    use uds_session::{
        Address, AddressExtension, Ai, Mtype, SResult, TaType, TransportError,
    };

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);
    const ENTITY: LogicalAddress = LogicalAddress(0x0001);

    fn ai_with(mtype: Mtype) -> Ai {
        Ai {
            mtype,
            sa: Address(0x0E00),
            ta: Address(0x0E80),
            ta_type: TaType::Physical,
        }
    }

    fn from_tester() -> Ai {
        Ai {
            mtype: Mtype::Diag,
            sa: Address(0x0E00),
            ta: Address(0x0001),
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
        assert_eq!(target_of(ai_with(Mtype::Diag)).map(|a| a.0), Ok(0x0E80));
        assert_eq!(
            target_of(ai_with(Mtype::SecureDiag)).map(|a| a.0),
            Ok(0x0E80)
        );
    }

    /// REQ 4.4 Table 5: `T_TAtype` is `DoIP_TAtype`, both ways.
    #[test]
    fn the_target_address_type_maps_both_ways() {
        for ta_type in [TaType::Physical, TaType::Functional] {
            assert_eq!(from_doip_ta_type(to_doip_ta_type(ta_type)), ta_type);
        }
        assert_eq!(
            to_doip_ta_type(TaType::Functional),
            simple_doip::TaType::Functional
        );
    }

    /// REQ 4.4 Table 5: `T_Result` is `DoIP_Result`. `DoIP_OK` is the session
    /// layer's `S_OK`, and every error stays distinct, numbered by its place in
    /// ISO 13400-2:2019 8.2.5's normative order.
    #[test]
    fn every_doip_result_maps_to_a_distinct_result() {
        let errors = [
            DoIpResult::HdrError,
            DoIpResult::TimeoutA,
            DoIpResult::UnknownSa,
            DoIpResult::InvalidSa,
            DoIpResult::UnknownTa,
            DoIpResult::MessageTooLarge,
            DoIpResult::OutOfMemory,
            DoIpResult::TargetUnreachable,
            DoIpResult::NoLink,
            DoIpResult::NoSocket,
            DoIpResult::Error,
        ];
        assert_eq!(s_result(DoIpResult::Ok), SResult::Ok);
        for (position, error) in (1..).zip(errors) {
            assert_eq!(
                s_result(error),
                SResult::Transport(TransportError(position)),
                "{error:?}"
            );
        }
    }

    /// REQ 4.3 Table 4: `DoIP_Data.indication` is `T_Data.ind`, and the PDU is
    /// located in the caller's buffer rather than copied out of it.
    #[test]
    fn an_indication_is_a_data_indication_located_in_the_buffer() {
        let buffer = [0x00, 0x22, 0xF1, 0x90, 0x00];
        let event = EntityEvent::Indication {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: simple_doip::TaType::Physical,
            pdu: buffer.get(1..4).unwrap_or_default(),
        };
        assert_eq!(
            classify(event, buffer.as_ptr().addr()),
            Ok(Some(Inbound::Ind {
                connection: ConnectionId::new(0),
                ai: from_tester(),
                at: 1..4,
            }))
        );
    }

    /// A truncated indication keeps the whole message's length from the header,
    /// so the driver can tell a fragment from a message.
    #[test]
    fn a_truncated_indication_is_too_long_with_its_declared_length() {
        let buffer = [0x2E, 0xF1, 0x90];
        let event = EntityEvent::IndicationTruncated {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: simple_doip::TaType::Physical,
            pdu: &buffer,
            length: 40,
        };
        assert_eq!(
            classify(event, buffer.as_ptr().addr()),
            Ok(Some(Inbound::TooLong {
                connection: ConnectionId::new(0),
                ai: from_tester(),
                at: 0..3,
                declared: 40,
            }))
        );
    }

    /// REQ 4.3 Table 4: `DoIP_Data.confirm` is `T_Data.conf`, with the confirmed
    /// request's own addressing: from the entity, to the tester.
    #[test]
    fn a_confirm_is_a_data_confirmation_with_the_requests_addressing() {
        let event = EntityEvent::Confirm {
            sa: ENTITY,
            ta: TESTER,
            ta_type: simple_doip::TaType::Physical,
            result: DoIpResult::NoSocket,
        };
        assert_eq!(
            classify(event, 0),
            Ok(Some(Inbound::Conf {
                ai: Ai {
                    mtype: Mtype::Diag,
                    sa: Address(0x0001),
                    ta: Address(0x0E00),
                    ta_type: TaType::Physical,
                },
                result: SResult::Transport(TransportError(10)),
            }))
        );
    }

    /// A periodic response, which a server sends rather than receives (ISO
    /// 14229-5:2022 REQ 7.16), is ignored rather than mistaken for a request.
    #[test]
    fn an_unmodelled_payload_is_ignored() {
        let data = [0x01, 0x02];
        let event = EntityEvent::Unmodelled {
            connection: ConnectionId::new(0),
            payload_type: super::PERIODIC_RESPONSE_PAYLOAD_TYPE,
            data: &data,
        };
        assert_eq!(classify(event, data.as_ptr().addr()), Ok(None));
    }

    /// One too long for the buffers is ignored too: its payload type is what decides.
    #[test]
    fn an_unmodelled_payload_too_long_for_the_buffer_is_ignored() {
        let data = [0x01, 0x02];
        let event = EntityEvent::UnmodelledTruncated {
            connection: ConnectionId::new(0),
            payload_type: super::PERIODIC_RESPONSE_PAYLOAD_TYPE,
            data: &data,
            length: 4_096,
        };
        assert_eq!(classify(event, data.as_ptr().addr()), Ok(None));
    }

    /// An entity that reports a PDU outside the buffer it was lent has broken its
    /// contract, and that is reported rather than turned into a range.
    #[test]
    fn a_pdu_outside_the_buffer_is_refused() {
        let elsewhere = [0x3E, 0x00];
        let event = EntityEvent::Indication {
            connection: ConnectionId::new(0),
            sa: TESTER,
            ta: ENTITY,
            ta_type: simple_doip::TaType::Physical,
            pdu: &elsewhere,
        };
        let after = elsewhere.as_ptr().addr().wrapping_add(1);
        assert_eq!(classify(event, after), Err(PduOutsideBuffer));
    }
}
