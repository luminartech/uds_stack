//! The one sink a handler writes into.
//!
//! ``UDSSVC_ARCH_0017``. Neither `dyn` nor generic: [`crate::storage`] owns the response
//! buffer, so the sink is never the caller's and there is nothing for a type parameter to
//! vary. Dispatch stays fully monomorphised, with no vtable.

use automotive_wire_codec::{InsufficientBuffer, Sink, WriteError};

/// A bounded writer over this crate's response buffer.
///
/// The bound is `min(buffer.len(), max(outbound_max, 3))` — see [`Self::MIN_BOUND`].
/// Both terms are this crate's, so nothing is fabricated and nothing is asked of a layer
/// that does not know it — which is what ``UDSSVC_ARCH_0017`` requires after
/// ISO 13400-2:2019 Table 11 made *Max. data size* optional.
///
/// The floor is three bytes because a negative response, `7F <sid> <nrc>`, is exactly that
/// long and must always fit. A write that would cross the bound is refused whole, and the
/// sink records the refusal before returning the error, so it is not lost if a handler
/// ignores the result. Such a refusal means the response was too long, not that nothing
/// was written; the pipeline reads it after the handler returns and answers
/// `responseTooLong` (0x14) in place of the partial response.
///
/// The bound is not a field. [`Self::new`] truncates the buffer to it, so "written never
/// exceeds the limit, which never exceeds the buffer" is one slice length rather than an
/// ordering between two numbers that each method had to be trusted to maintain.
///
/// # Examples
///
/// ```
/// use uds_services::{ResponseSink, Sink};
///
/// let mut buffer = [0_u8; 8];
/// // The peer advertised room for five bytes, so five is the bound, not eight.
/// let mut sink = ResponseSink::new(&mut buffer, Some(5));
/// assert_eq!(sink.remaining(), 5);
///
/// sink.write_all(&[0x62, 0xF1, 0x90])?;
/// assert_eq!(sink.written_bytes(), &[0x62, 0xF1, 0x90]);
/// assert_eq!(sink.remaining(), 2);
///
/// // A write that would cross the bound is refused whole: nothing is recorded.
/// assert!(sink.write_all(&[0x00; 3]).is_err());
/// assert_eq!(sink.written(), 3);
/// # Ok::<(), uds_services::WriteError>(())
/// ```
#[derive(Debug)]
pub struct ResponseSink<'a> {
    buffer: &'a mut [u8],
    written: usize,
    refused: bool,
}

impl<'a> ResponseSink<'a> {
    /// The smallest message the protocol admits: a negative response, `7F <sid> <nrc>`.
    pub const MIN_BOUND: usize = 3;

    /// A sink over `buffer`, bounded at the peer's `outbound_max` where one is advertised,
    /// but never below three bytes: a peer that cannot receive a negative response cannot
    /// take part in UDS at all, so a bound below [`Self::MIN_BOUND`] is raised to it. The
    /// buffer itself is still the hard limit.
    #[must_use]
    pub fn new(buffer: &'a mut [u8], outbound_max: Option<usize>) -> Self {
        let limit = outbound_max.map_or(buffer.len(), |max| {
            max.max(Self::MIN_BOUND).min(buffer.len())
        });
        let buffer = buffer
            .split_at_mut_checked(limit)
            .map_or(&mut [][..], |(within, _beyond)| within);
        Self {
            buffer,
            written: 0,
            refused: false,
        }
    }

    /// Whether any write was refused since creation or the last rewind.
    ///
    /// ``UDSSVC_ARCH_0017`` — the pipeline reads this after the handler returns and
    /// settles `responseTooLong` (0x14). A handler ignores `write_all`'s result; the
    /// refusal is recorded here and decided there.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "consumed by the pipeline's settle (Task 10)")
    )]
    pub(crate) const fn refused(&self) -> bool {
        self.refused
    }

    /// Discard everything written and clear the refusal, so a negative response can
    /// replace a partial positive one. Crate-private: a handler that could rewind could
    /// erase the identifier the pipeline wrote ahead of it and return `Ok`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "consumed by the pipeline's settle (Task 10)")
    )]
    pub(crate) fn rewind(&mut self) {
        self.written = 0;
        self.refused = false;
    }

    /// How many bytes the handler has written.
    #[must_use]
    pub const fn written(&self) -> usize {
        self.written
    }

    /// What the handler wrote.
    ///
    /// The driver needs this because [`Self::new`] moves the slice in, leaving no
    /// other route back to the bytes it must transmit.
    #[must_use]
    pub fn written_bytes(&self) -> &[u8] {
        self.buffer.get(..self.written).unwrap_or(&[])
    }

    /// How many more bytes will be accepted before the bound is hit.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.written)
    }
}

impl Sink for ResponseSink<'_> {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let available = self.buffer.len();
        let insufficient = |needed_at_least| {
            Err(WriteError::Insufficient(InsufficientBuffer {
                needed_at_least,
                available,
            }))
        };
        let Some(end) = self.written.checked_add(bytes.len()) else {
            self.refused = true;
            return insufficient(usize::MAX);
        };
        let Some(room) = self.buffer.get_mut(self.written..end) else {
            self.refused = true;
            return insufficient(end);
        };
        room.copy_from_slice(bytes);
        self.written = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ResponseSink;
    use automotive_wire_codec::{Sink, WriteError};
    use uds_protocol::{DtcRecord, DtcStatusMask, Encode};

    /// `uds_protocol`'s types encode into an `automotive_wire_codec::Sink`, and this is
    /// one — so a handler writes a value and its length cannot disagree with its bytes.
    /// Both traits are re-exported from the crate root so reaching this needs no third
    /// dependency.
    #[test]
    fn a_protocol_value_encodes_straight_into_the_sink() {
        let mut buf = [0_u8; 8];
        let mut sink = ResponseSink::new(&mut buf, None);

        let written = DtcRecord::new(0xC0, 0x01, 0x23)
            .encode(&mut sink)
            .and_then(|n| Ok(n + DtcStatusMask::TestFailed.encode(&mut sink)?));

        // `uds_protocol::Error` is not `PartialEq`, so compare the success side.
        assert_eq!(written.ok(), Some(4));
        assert_eq!(sink.written_bytes(), &[0xC0, 0x01, 0x23, 0x01]);
    }

    /// ``UDSSVC_ARCH_0017`` — with no peer advertisement the buffer alone bounds the
    /// response, which is a correct outcome rather than a degraded one.
    #[test]
    fn an_unadvertised_maximum_leaves_the_buffer_as_the_bound() {
        let mut buf = [0_u8; 8];
        assert_eq!(ResponseSink::new(&mut buf, None).remaining(), 8);
    }

    /// A peer that advertised less lowers the bound; one that advertised more does
    /// not raise it. The buffer is physical and the advertisement is a claim.
    #[test]
    fn the_smaller_of_the_two_bounds_wins() {
        let mut buf = [0_u8; 8];
        assert_eq!(ResponseSink::new(&mut buf, Some(3)).remaining(), 3);
        let mut buf = [0_u8; 8];
        assert_eq!(ResponseSink::new(&mut buf, Some(64)).remaining(), 8);
    }

    /// `written_bytes` is how the driver reaches what it must transmit: `new` moves
    /// the slice in, so there is no other route back to it.
    #[test]
    #[allow(clippy::unwrap_used, reason = "testing success path")]
    fn written_bytes_is_what_the_driver_transmits() {
        let mut buf = [0_u8; 8];
        let mut sink = ResponseSink::new(&mut buf, None);
        sink.write_all(&[0x62, 0xF1, 0x90]).unwrap();
        assert_eq!(sink.written(), 3);
        assert_eq!(sink.written_bytes(), &[0x62, 0xF1, 0x90]);
        assert_eq!(sink.remaining(), 5);
    }

    /// Over-running is the failure the pipeline turns into responseTooLong (0x14).
    /// `needed_at_least` is a lower bound by awc's own definition, and 0x14 carries
    /// no length, so a lower bound is sufficient.
    #[test]
    #[allow(clippy::unwrap_used, reason = "testing success path of first write")]
    #[allow(clippy::panic, reason = "testing failure path")]
    fn overrunning_the_bound_reports_the_counts() {
        let mut buf = [0_u8; 4];
        let mut sink = ResponseSink::new(&mut buf, None);
        sink.write_all(&[0x62, 0xF1]).unwrap();
        let Err(WriteError::Insufficient(e)) = sink.write_all(&[0; 5]) else {
            panic!("an over-long write must fail with Insufficient")
        };
        assert_eq!((e.needed_at_least, e.available), (7, 4));
    }

    /// A failed write records nothing: `write_all` is all-or-nothing.
    #[test]
    fn a_failed_write_records_nothing() {
        let mut buf = [0_u8; 4];
        let mut sink = ResponseSink::new(&mut buf, None);
        assert!(sink.write_all(&[0; 5]).is_err());
        assert_eq!(sink.written(), 0);
    }

    /// Spec §3.3 — ISO 14229-1 fixes a negative response at three bytes, so a bound
    /// below three is not one the protocol admits: it is raised to three.
    #[test]
    fn the_bound_is_never_below_three_bytes() {
        let mut buffer = [0_u8; 8];
        let sink = ResponseSink::new(&mut buffer, Some(1));
        assert_eq!(sink.remaining(), 3);
        let mut tiny = [0_u8; 2];
        let sink = ResponseSink::new(&mut tiny, Some(1));
        assert_eq!(sink.remaining(), 2); // the buffer itself is the hard limit
    }

    /// ``UDSSVC_ARCH_0017`` — a refused write is recorded for the pipeline to read; the
    /// handler need not report it. Rewinding discards everything written.
    #[test]
    fn a_refused_write_is_recorded_and_rewind_discards() {
        let mut buffer = [0_u8; 4];
        let mut sink = ResponseSink::new(&mut buffer, None);
        assert!(!sink.refused());
        let _ = sink.write_all(&[1, 2, 3]);
        let _ = sink.write_all(&[4, 5]);
        assert!(sink.refused());
        assert_eq!(sink.written(), 3);
        sink.rewind();
        assert_eq!(sink.written(), 0);
        assert!(!sink.refused());
        assert_eq!(sink.remaining(), 4);
    }
}
