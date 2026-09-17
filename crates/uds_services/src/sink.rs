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
#[derive(Debug)]
pub struct ResponseSink<'a> {
    buffer: &'a mut [u8],
    limit: usize,
    written: usize,
}

impl<'a> ResponseSink<'a> {
    /// A sink over `buffer`, bounded also by the peer's advertisement where it made one.
    #[must_use]
    pub fn new(buffer: &'a mut [u8], outbound_max: Option<usize>) -> Self {
        let limit = match outbound_max {
            Some(max) if max < buffer.len() => max,
            _ => buffer.len(),
        };
        Self {
            buffer,
            limit,
            written: 0,
        }
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
    #[allow(
        clippy::indexing_slicing,
        reason = "written never exceeds limit, which never exceeds buffer.len()"
    )]
    pub fn written_bytes(&self) -> &[u8] {
        &self.buffer[..self.written]
    }

    /// How many more bytes will be accepted before the bound is hit.
    #[must_use]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "written never exceeds limit: write_all rejects first"
    )]
    pub const fn remaining(&self) -> usize {
        self.limit - self.written
    }
}

impl Sink for ResponseSink<'_> {
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "the checked add guards the sum before either use"
    )]
    #[allow(
        clippy::indexing_slicing,
        reason = "end is proven <= limit <= buffer.len() by the guard above"
    )]
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        let Some(end) = self.written.checked_add(bytes.len()) else {
            return Err(WriteError::Insufficient(InsufficientBuffer {
                needed_at_least: usize::MAX,
                available: self.limit,
            }));
        };
        if end > self.limit {
            return Err(WriteError::Insufficient(InsufficientBuffer {
                needed_at_least: end,
                available: self.limit,
            }));
        }
        self.buffer[self.written..end].copy_from_slice(bytes);
        self.written = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ResponseSink;
    use automotive_wire_codec::{Sink, WriteError};

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
