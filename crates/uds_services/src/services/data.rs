//! Data transmission — ISO 14229-1:2020 clause 11.

use crate::{DataIdentifier, ResponseSink, SecurityLevel};
use uds_protocol::{DiagnosticSessionType, NegativeResponseCode};

/// `ReadDataByIdentifier` (0x22).
///
/// ``UDSSVC_ARCH_0008`` — a request naming several identifiers of which some are
/// supported yields a **positive** response carrying the supported ones, which is why the
/// response buffer must hold the worst case rather than one record.
pub trait ReadDataByIdentifier {
    /// This application's data identifier enumeration.
    type Did: DataIdentifier;

    /// ``UDSSVC_ARCH_0033``.
    const MAY_RESPOND_PENDING: bool;

    /// The most identifiers this server accepts in one request.
    ///
    /// Clause 11.2.1 permits a server to limit the count; this is that limit, and a
    /// request naming more is rejected with `incorrectMessageLengthOrInvalidFormat`
    /// (0x13). With [`DataIdentifier::MAX_RECORD_LEN`] it bounds the response buffer.
    const MAX_DIDS_PER_REQUEST: usize;

    /// Write `did`'s data record into `out`. The identifier is written by the pipeline.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for an identifier that is known but currently
    /// unreadable. One this application does not define never reaches here:
    /// [`DataIdentifier::from_u16`] returning `None` produces `requestOutOfRange` (0x31).
    ///
    /// A refused write to `out` needs no handling: the sink records the refusal and the
    /// pipeline answers `responseTooLong` (0x14) in place of the response, so the write's
    /// `Result` may be discarded. See [`ResponseSink`].
    fn read(
        &mut self,
        did: Self::Did,
        out: &mut ResponseSink<'_>,
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}

/// `WriteDataByIdentifier` (0x2E).
///
/// ISO 14229-1:2020 clause 11.7, Figure 26 — the pipeline settles, in order: a request
/// with no data record (0x13), an identifier [`DataIdentifier::from_u16`] rejects or
/// [`Self::writable_in`] refuses (0x31), a record [`DataIdentifier::split_record`] finds
/// short or followed by more bytes (0x13), a locked [`Self::required_level`] (0x33), and a
/// record `split_record` finds malformed (0x31). Only then is [`Self::write`] asked.
pub trait WriteDataByIdentifier {
    /// This application's data identifier enumeration.
    type Did: DataIdentifier;

    /// ``UDSSVC_ARCH_0033`` — a write reaching non-volatile memory usually cannot accept
    /// further requests while it completes.
    const MAY_RESPOND_PENDING: bool;

    /// Whether `did` may be written in the `active` session.
    ///
    /// Figure 26, "DID supports service 2E in active session?" — a `false` settles the
    /// request `requestOutOfRange` (0x31), as clause 11.7.4 has it for a read-only
    /// identifier, before the record's length is checked.
    ///
    /// # Arguments
    ///
    /// * `did` - the identifier the request names; see [`Self::Did`].
    /// * `active` - the session the server is in when the request arrives; see
    ///   [`DiagnosticSessionType`].
    fn writable_in(&self, did: Self::Did, active: DiagnosticSessionType) -> bool;

    /// The security level writing `did` requires unlocked, or `None` where it requires
    /// none.
    ///
    /// Figure 26, "DID security check OK?" — where the level this crate holds unlocked
    /// is not the one returned, the request settles `securityAccessDenied` (0x33) without
    /// [`Self::write`] being asked.
    ///
    /// # Arguments
    ///
    /// * `did` - the identifier the request names; see [`Self::Did`].
    fn required_level(&self, did: Self::Did) -> Option<SecurityLevel>;

    /// Store `record` as `did`'s data record.
    ///
    /// # Arguments
    ///
    /// * `did` - an identifier [`Self::writable_in`] accepted and whose
    ///   [`Self::required_level`] is unlocked; see [`Self::Did`].
    /// * `record` - exactly the record [`DataIdentifier::split_record`] took.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a write that cannot be performed, clause 11.7.4:
    /// `conditionsNotCorrect` (0x22), `requestOutOfRange` (0x31) for a value the
    /// application rejects, or `generalProgrammingFailure` (0x72) for a write that failed.
    fn write(
        &mut self,
        did: Self::Did,
        record: &[u8],
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}
