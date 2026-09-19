//! The one sink a handler writes into.
//!
//! ``UDSSVC_ARCH_0017``. Neither `dyn` nor generic: a `<S: Sink>` parameter was only
//! ever needed because the sink might be the caller's, and under [`crate::storage`] it
//! cannot be — this crate owns the response buffer. Removing it takes a type parameter
//! off every service trait, every generated impl and [`crate::Server`] itself, and
//! costs nothing, because dispatch stays fully monomorphised with no vtable.

use automotive_wire_codec::{InsufficientBuffer, Sink, WriteError};

/// A bounded writer over this crate's response buffer.
///
/// The bound is `min(buffer.len(), outbound_max)`. Both terms are this crate's, so
/// nothing is fabricated and nothing is asked of a layer that does not know it —
/// which is what ``UDSSVC_ARCH_0017`` requires after ISO 13400-2:2019 Table 11 made
/// *Max. data size* optional.
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
}

impl<'a> ResponseSink<'a> {
    /// A sink over `buffer`, bounded also by the peer's advertisement where it made one.
    #[must_use]
    pub fn new(buffer: &'a mut [u8], outbound_max: Option<usize>) -> Self {
        let limit = outbound_max.map_or(buffer.len(), |max| max.min(buffer.len()));
        let buffer = buffer
            .split_at_mut_checked(limit)
            .map_or(&mut [][..], |(within, _beyond)| within);
        Self { buffer, written: 0 }
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
            return insufficient(usize::MAX);
        };
        let Some(room) = self.buffer.get_mut(self.written..end) else {
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
}
