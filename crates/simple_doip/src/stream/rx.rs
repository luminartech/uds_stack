use super::DoesNotFit;
use crate::messages::{Header, MessageError};
use crate::wire::Decode;
use crate::{RawFrame, try_frame};

/// Bytes read from the socket and not yet consumed, holding at most one frame of `N`
/// bytes, plus how much of an oversized frame is still to be skipped.
#[derive(Debug)]
pub(crate) struct RxBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
    discard: usize,
}

/// What [`RxBuffer::next`] found at the front of the buffered bytes.
#[derive(Debug, PartialEq)]
pub(crate) enum Next<'a> {
    /// A whole frame, and the bytes it occupies.
    Frame(RawFrame<'a>, usize),
    /// The start of a frame longer than the buffer, which is full of it.
    Oversized { header: Header, head: &'a [u8] },
    /// Less than a whole frame: read more.
    NeedMore,
}

impl<const N: usize> RxBuffer<N> {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            discard: 0,
        }
    }

    /// How many bytes are read and not yet consumed.
    pub(crate) fn buffered_len(&self) -> usize {
        self.len
    }

    pub(crate) fn free(&mut self) -> &mut [u8] {
        self.buf.get_mut(self.len..).unwrap_or_default()
    }

    /// Records that `read` bytes were read into [`Self::free`].
    pub(crate) fn filled(&mut self, read: usize) {
        let read = read.min(N.saturating_sub(self.len));
        let skipped = read.min(self.discard);
        let kept = read.saturating_sub(skipped);
        let start = self.len.saturating_add(skipped);
        self.buf
            .copy_within(start..start.saturating_add(kept), self.len);
        self.discard = self.discard.saturating_sub(skipped);
        self.len = self.len.saturating_add(kept);
    }

    pub(crate) fn next(&self) -> Result<Next<'_>, MessageError> {
        let buffered = self.buf.get(..self.len).unwrap_or_default();
        if let Some((frame, consumed)) = try_frame(buffered)? {
            return Ok(Next::Frame(frame, consumed));
        }
        if self.len < N || N <= Header::SIZE {
            return Ok(Next::NeedMore);
        }
        let (header, head) = Header::decode(buffered)?;
        Ok(Next::Oversized { header, head })
    }

    pub(crate) fn consume(&mut self, consumed: usize) {
        let consumed = consumed.min(self.len);
        self.buf.copy_within(consumed..self.len, 0);
        self.len = self.len.saturating_sub(consumed);
    }

    /// Drops the buffered start of the frame `header` describes, and the rest of it as
    /// it arrives.
    pub(crate) fn skip_frame(&mut self, header: &Header) {
        let buffered = self.len.saturating_sub(Header::SIZE);
        self.discard = usize::try_from(header.payload_length)
            .unwrap_or(usize::MAX)
            .saturating_sub(buffered);
        self.len = 0;
    }

    /// The free space up to the end of the frame being read: the rest of its header,
    /// then the rest of its payload, so that nothing after the frame is read before the
    /// frame is handled. Empty once a whole frame, or a header that does not decode,
    /// is buffered.
    pub(crate) fn free_to_frame_end(&mut self) -> &mut [u8] {
        let end = if self.discard > 0 {
            self.len.saturating_add(self.discard)
        } else if self.len < Header::SIZE {
            Header::SIZE
        } else {
            self.header().map_or(self.len, |header| {
                usize::try_from(header.payload_length)
                    .map_or(usize::MAX, |length| Header::SIZE.saturating_add(length))
            })
        };
        let start = self.len.min(N);
        self.buf
            .get_mut(start..end.clamp(start, N))
            .unwrap_or_default()
    }

    /// The header of the frame being read, once all of it is buffered.
    pub(crate) fn header(&self) -> Result<Header, MessageError> {
        let (header, _) = Header::decode(self.buf.get(..self.len).unwrap_or_default())?;
        Ok(header)
    }

    /// Whether nothing is buffered and no skip is under way.
    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.len == 0 && self.discard == 0
    }

    /// Exchanges contents with `other`, of another capacity.
    ///
    /// # Errors
    ///
    /// [`DoesNotFit`] where either side holds more than the other can, leaving both
    /// unchanged.
    pub(crate) fn swap<const M: usize>(
        &mut self,
        other: &mut RxBuffer<M>,
    ) -> Result<(), DoesNotFit> {
        let span = self.len.max(other.len);
        let (Some(mine), Some(theirs)) =
            (self.buf.get_mut(..span), other.buf.get_mut(..span))
        else {
            return Err(DoesNotFit);
        };
        mine.swap_with_slice(theirs);
        core::mem::swap(&mut self.len, &mut other.len);
        core::mem::swap(&mut self.discard, &mut other.discard);
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.discard = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalAddress;
    use crate::messages::{Message, PayloadType, ProtocolVersion};
    use crate::wire::{Encode, SliceSink};
    use core::slice::SliceIndex;

    fn encode(message: &Message<'_>, out: &mut [u8]) -> usize {
        message.encode(&mut SliceSink::new(out)).unwrap()
    }

    fn diagnostic(pdu: &[u8], out: &mut [u8]) -> usize {
        let message = Message::diagnostic_message(
            ProtocolVersion::V2019,
            LogicalAddress(0x0001),
            LogicalAddress(0x0E00),
            pdu,
        );
        encode(&message, out)
    }

    fn feed<const N: usize>(rx: &mut RxBuffer<N>, bytes: &[u8]) {
        for byte in bytes {
            *rx.free().first_mut().unwrap() = *byte;
            rx.filled(1);
        }
    }

    fn part<R: SliceIndex<[u8], Output = [u8]>>(bytes: &[u8], range: R) -> &[u8] {
        bytes.get(range).unwrap()
    }

    fn part_mut<R: SliceIndex<[u8], Output = [u8]>>(
        bytes: &mut [u8],
        range: R,
    ) -> &mut [u8] {
        bytes.get_mut(range).unwrap()
    }

    fn plus(a: usize, b: usize) -> usize {
        a.checked_add(b).unwrap()
    }

    #[test]
    fn a_frame_read_a_byte_at_a_time_is_whole_only_at_its_last_byte() {
        let mut wire = [0u8; 32];
        let len = diagnostic(&[0x62, 0xF1, 0x90], &mut wire);
        let mut rx = RxBuffer::<32>::new();

        let last = len.checked_sub(1).unwrap();
        feed(&mut rx, part(&wire, ..last));
        assert_eq!(rx.next().unwrap(), Next::NeedMore);

        feed(&mut rx, part(&wire, last..len));
        let Next::Frame(frame, consumed) = rx.next().unwrap() else {
            panic!("expected a frame");
        };
        assert_eq!(frame.header.payload_type, PayloadType::DiagnosticMessage);
        assert_eq!(frame.payload, part(&wire, 8..len));
        assert_eq!(consumed, len);
    }

    #[test]
    fn two_frames_in_one_read_come_out_in_order() {
        let mut wire = [0u8; 64];
        let first = diagnostic(&[0x01], &mut wire);
        let second = diagnostic(&[0x02, 0x03], part_mut(&mut wire, first..));
        let both = plus(first, second);
        let mut rx = RxBuffer::<64>::new();
        part_mut(rx.free(), ..both).copy_from_slice(part(&wire, ..both));
        rx.filled(both);

        let Next::Frame(frame, consumed) = rx.next().unwrap() else {
            panic!("expected the first frame");
        };
        assert_eq!(frame.payload.last(), Some(&0x01));
        rx.consume(consumed);

        let Next::Frame(frame, consumed) = rx.next().unwrap() else {
            panic!("expected the second frame");
        };
        assert_eq!(frame.payload.get(4..), Some(&[0x02, 0x03][..]));
        rx.consume(consumed);
        assert_eq!(rx.next().unwrap(), Next::NeedMore);
    }

    #[test]
    fn an_oversized_frame_is_reported_once_the_buffer_is_full_then_skipped() {
        let mut wire = [0u8; 64];
        let big = diagnostic(&[0xAA; 30], &mut wire);
        let small = diagnostic(&[0x55], part_mut(&mut wire, big..));
        let mut rx = RxBuffer::<24>::new();

        feed(&mut rx, part(&wire, ..23));
        assert_eq!(rx.next().unwrap(), Next::NeedMore);
        feed(&mut rx, part(&wire, 23..24));
        let Next::Oversized { header, head } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        assert_eq!(header.payload_length, 34);
        assert_eq!(head, part(&wire, 8..24));

        rx.skip_frame(&header);
        feed(&mut rx, part(&wire, 24..plus(big, small)));
        let Next::Frame(frame, _) = rx.next().unwrap() else {
            panic!("expected the frame after the oversized one");
        };
        assert_eq!(frame.payload, part(&wire, plus(big, 8)..plus(big, small)));
    }

    #[test]
    fn a_skip_is_spread_over_many_reads() {
        let mut wire = [0u8; 64];
        let big = diagnostic(&[0xAA; 30], &mut wire);
        let small = diagnostic(&[0x55], part_mut(&mut wire, big..));
        let mut rx = RxBuffer::<24>::new();
        feed(&mut rx, part(&wire, ..24));
        let Next::Oversized { header, .. } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        rx.skip_frame(&header);

        let mut rest = part(&wire, 24..plus(big, small));
        while !rest.is_empty() {
            let free = rx.free();
            let read = free.len().min(rest.len()).min(7);
            part_mut(free, ..read).copy_from_slice(part(rest, ..read));
            rx.filled(read);
            rest = part(rest, read..);
        }
        let Next::Frame(frame, _) = rx.next().unwrap() else {
            panic!("expected the frame after the oversized one");
        };
        assert_eq!(frame.payload.last(), Some(&0x55));
    }

    /// A length the peer chose at the very top of the `u32` range is skipped like any
    /// other, with no arithmetic overflowing on a 32-bit target.
    #[test]
    fn the_largest_declared_length_is_skipped_without_overflow() {
        let mut rx = RxBuffer::<24>::new();
        feed(&mut rx, &[0x03, 0xFC, 0x80, 0x01, 0xFF, 0xFF, 0xFF, 0xFF]);
        feed(&mut rx, &[0xAA; 16]);
        let Next::Oversized { header, .. } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };

        rx.skip_frame(&header);

        assert_eq!(rx.discard, 0xFFFF_FFEF);
        feed(&mut rx, &[0xAA; 30]);
        assert_eq!(rx.next().unwrap(), Next::NeedMore);
        assert_eq!(rx.len, 0);
    }

    fn read_to_frame_end<const N: usize>(rx: &mut RxBuffer<N>, wire: &mut &[u8]) -> usize {
        let free = rx.free_to_frame_end();
        let read = free.len().min(wire.len());
        part_mut(free, ..read).copy_from_slice(part(wire, ..read));
        rx.filled(read);
        *wire = part(wire, read..);
        read
    }

    #[test]
    fn reads_to_the_frame_end_stop_at_the_frame_boundary() {
        let mut wire = [0u8; 64];
        let first = diagnostic(&[0x01, 0x02], &mut wire);
        let second = diagnostic(&[0x03], part_mut(&mut wire, first..));
        let mut rest = part(&wire, ..plus(first, second));
        let mut rx = RxBuffer::<64>::new();

        assert_eq!(read_to_frame_end(&mut rx, &mut rest), 8);
        assert_eq!(
            read_to_frame_end(&mut rx, &mut rest),
            first.checked_sub(8).unwrap()
        );
        assert_eq!(read_to_frame_end(&mut rx, &mut rest), 0);
        let Next::Frame(frame, consumed) = rx.next().unwrap() else {
            panic!("expected the first frame");
        };
        assert_eq!(frame.payload.get(4..), Some(&[0x01, 0x02][..]));
        rx.consume(consumed);
        assert_eq!(rest.len(), second);
    }

    #[test]
    fn a_skipped_frame_is_read_to_its_end_and_no_further() {
        let mut wire = [0u8; 96];
        let big = diagnostic(&[0xAA; 50], &mut wire);
        let small = diagnostic(&[0x55], part_mut(&mut wire, big..));
        let mut rest = part(&wire, ..plus(big, small));
        let mut rx = RxBuffer::<24>::new();
        read_to_frame_end(&mut rx, &mut rest);
        rx.skip_frame(&rx.header().unwrap());

        while !rx.is_idle() {
            read_to_frame_end(&mut rx, &mut rest);
        }
        assert_eq!(rest.len(), small);
    }

    #[test]
    fn swapping_moves_contents_between_capacities() {
        let mut wire = [0u8; 32];
        let len = diagnostic(&[0x01], &mut wire);
        let mut small = RxBuffer::<16>::new();
        let mut large = RxBuffer::<64>::new();
        feed(&mut large, part(&wire, ..len));

        small.swap(&mut large).unwrap();

        assert!(large.is_idle());
        assert!(matches!(small.next().unwrap(), Next::Frame(..)));
    }

    #[test]
    fn swapping_more_than_fits_changes_nothing() {
        let mut small = RxBuffer::<8>::new();
        let mut large = RxBuffer::<64>::new();
        feed(&mut large, &[0x03; 9]);

        assert_eq!(small.swap(&mut large), Err(DoesNotFit));
        assert_eq!(large.len, 9);
        assert!(small.is_idle());
    }

    #[test]
    fn a_bad_protocol_pattern_is_framing_fatal() {
        let mut rx = RxBuffer::<16>::new();
        feed(&mut rx, &[0x03, 0xFD, 0x80, 0x01, 0, 0, 0, 4]);
        assert!(rx.next().unwrap_err().is_framing_fatal());
    }

    #[test]
    fn clearing_forgets_a_partial_frame_and_a_pending_skip() {
        let mut wire = [0u8; 64];
        let big = diagnostic(&[0xAA; 30], &mut wire);
        let mut rx = RxBuffer::<24>::new();
        feed(&mut rx, part(&wire, ..24));
        let Next::Oversized { header, .. } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        rx.skip_frame(&header);
        rx.clear();

        let small = diagnostic(&[0x55], part_mut(&mut wire, big..));
        feed(&mut rx, part(&wire, big..plus(big, small)));
        assert!(matches!(rx.next().unwrap(), Next::Frame(..)));
    }
}
