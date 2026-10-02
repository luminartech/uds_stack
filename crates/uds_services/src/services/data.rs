//! Data transmission — ISO 14229-1:2020 clause 11.

use crate::{DataIdentifier, ResponseSink};
use uds_protocol::NegativeResponseCode;

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
pub trait WriteDataByIdentifier {
    /// This application's data identifier enumeration.
    type Did: DataIdentifier;

    /// ``UDSSVC_ARCH_0033`` — a write reaching non-volatile memory usually cannot accept
    /// further requests while it completes.
    const MAY_RESPOND_PENDING: bool;

    /// Store `record` as `did`'s data record.
    ///
    /// # Errors
    ///
    /// The [`NegativeResponseCode`] for a read-only identifier, a rejected value, or a
    /// write that failed.
    fn write(
        &mut self,
        did: Self::Did,
        record: &[u8],
    ) -> impl core::future::Future<Output = Result<(), NegativeResponseCode>>;
}
