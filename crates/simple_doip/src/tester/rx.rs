// The lengths here come from the peer, so their arithmetic is held to the standard the
// crate root relaxes.
#![deny(clippy::arithmetic_side_effects)]

use crate::messages::{Header, MessageError};
use crate::wire::Decode;
use crate::{RawFrame, try_frame};

/// Bytes read from the socket and not yet consumed, holding at most one frame of `N`
/// bytes, plus how much of an oversized frame is still to be skipped.
#[derive(Debug)]
pub(super) struct RxBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
    discard: usize,
}

/// What the buffered bytes hold.
#[derive(Debug, PartialEq)]
pub(super) enum Next<'a> {
    /// A whole frame, and the bytes it occupies.
    Frame(RawFrame<'a>, usize),
    /// The start of a frame longer than the buffer, which is full of it.
    Oversized {
        header: Header,
        head: &'a [u8],
    },
    NeedMore,
}

impl<const N: usize> RxBuffer<N> {
    pub(super) const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            discard: 0,
        }
    }

    pub(super) fn free(&mut self) -> &mut [u8] {
        &mut self.buf[self.len..]
    }

    /// Records that `read` bytes were read into [`Self::free`].
    pub(super) fn filled(&mut self, read: usize) {
        let read = read.min(N.saturating_sub(self.len));
        let skipped = read.min(self.discard);
        let kept = read.saturating_sub(skipped);
        let start = self.len.saturating_add(skipped);
        self.buf
            .copy_within(start..start.saturating_add(kept), self.len);
        self.discard = self.discard.saturating_sub(skipped);
        self.len = self.len.saturating_add(kept);
    }

    pub(super) fn next(&self) -> Result<Next<'_>, MessageError> {
        let buffered = &self.buf[..self.len];
        if let Some((frame, consumed)) = try_frame(buffered)? {
            return Ok(Next::Frame(frame, consumed));
        }
        if self.len < N || N <= Header::SIZE {
            return Ok(Next::NeedMore);
        }
        let (header, head) = Header::decode(buffered)?;
        Ok(Next::Oversized { header, head })
    }

    pub(super) fn consume(&mut self, consumed: usize) {
        let consumed = consumed.min(self.len);
        self.buf.copy_within(consumed..self.len, 0);
        self.len = self.len.saturating_sub(consumed);
    }

    /// Drops the buffered start of the oversized frame `header` describes, and the rest
    /// of it as it arrives.
    pub(super) fn skip_oversized(&mut self, header: &Header) {
        let buffered = self.len.saturating_sub(Header::SIZE);
        self.discard = (header.payload_length as usize).saturating_sub(buffered);
        self.len = 0;
    }

    pub(super) fn clear(&mut self) {
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
            rx.free()[0] = *byte;
            rx.filled(1);
        }
    }

    #[test]
    fn a_frame_read_a_byte_at_a_time_is_whole_only_at_its_last_byte() {
        let mut wire = [0u8; 32];
        let len = diagnostic(&[0x62, 0xF1, 0x90], &mut wire);
        let mut rx = RxBuffer::<32>::new();

        feed(&mut rx, &wire[..len - 1]);
        assert_eq!(rx.next().unwrap(), Next::NeedMore);

        feed(&mut rx, &wire[len - 1..len]);
        let Next::Frame(frame, consumed) = rx.next().unwrap() else {
            panic!("expected a frame");
        };
        assert_eq!(frame.header.payload_type, PayloadType::DiagnosticMessage);
        assert_eq!(frame.payload, &wire[8..len]);
        assert_eq!(consumed, len);
    }

    #[test]
    fn two_frames_in_one_read_come_out_in_order() {
        let mut wire = [0u8; 64];
        let first = diagnostic(&[0x01], &mut wire);
        let second = diagnostic(&[0x02, 0x03], &mut wire[first..]);
        let mut rx = RxBuffer::<64>::new();
        rx.free()[..first + second].copy_from_slice(&wire[..first + second]);
        rx.filled(first + second);

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
        let small = diagnostic(&[0x55], &mut wire[big..]);
        let mut rx = RxBuffer::<24>::new();

        feed(&mut rx, &wire[..23]);
        assert_eq!(rx.next().unwrap(), Next::NeedMore);
        feed(&mut rx, &wire[23..24]);
        let Next::Oversized { header, head } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        assert_eq!(header.payload_length, 34);
        assert_eq!(head, &wire[8..24]);

        rx.skip_oversized(&header);
        feed(&mut rx, &wire[24..big + small]);
        let Next::Frame(frame, _) = rx.next().unwrap() else {
            panic!("expected the frame after the oversized one");
        };
        assert_eq!(frame.payload, &wire[big + 8..big + small]);
    }

    #[test]
    fn a_skip_is_spread_over_many_reads() {
        let mut wire = [0u8; 64];
        let big = diagnostic(&[0xAA; 30], &mut wire);
        let small = diagnostic(&[0x55], &mut wire[big..]);
        let mut rx = RxBuffer::<24>::new();
        feed(&mut rx, &wire[..24]);
        let Next::Oversized { header, .. } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        rx.skip_oversized(&header);

        let mut rest = &wire[24..big + small];
        while !rest.is_empty() {
            let free = rx.free();
            let read = free.len().min(rest.len()).min(7);
            free[..read].copy_from_slice(&rest[..read]);
            rx.filled(read);
            rest = &rest[read..];
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

        rx.skip_oversized(&header);

        assert_eq!(rx.discard, 0xFFFF_FFFF - 16);
        feed(&mut rx, &[0xAA; 30]);
        assert_eq!(rx.next().unwrap(), Next::NeedMore);
        assert_eq!(rx.len, 0);
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
        feed(&mut rx, &wire[..24]);
        let Next::Oversized { header, .. } = rx.next().unwrap() else {
            panic!("expected an oversized frame");
        };
        rx.skip_oversized(&header);
        rx.clear();

        let small = diagnostic(&[0x55], &mut wire[big..]);
        feed(&mut rx, &wire[big..big + small]);
        assert!(matches!(rx.next().unwrap(), Next::Frame(..)));
    }
}
