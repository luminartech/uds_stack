//! A code a table of ISO 13400-2:2019 reserves.

/// A single-byte code that the table it belongs to does not name. Only decoding
/// produces one, so a `Reserved` variant never holds a byte the table names, and a
/// value decoded from a byte encodes back to that byte.
///
/// ```compile_fail
/// use simple_doip::messages::{DiagnosticNackCode, ReservedCode};
/// // `ReservedCode` has no public constructor: a named byte cannot be smuggled in.
/// let aliased = DiagnosticNackCode::Reserved(ReservedCode(0x03));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ReservedCode(u8);

impl ReservedCode {
    pub(crate) const fn new(code: u8) -> Self {
        Self(code)
    }

    /// The byte on the wire.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<ReservedCode> for u8 {
    fn from(code: ReservedCode) -> Self {
        code.0
    }
}
