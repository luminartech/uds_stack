use super::DoesNotFit;
use crate::messages::{Message, MessageError};
use crate::wire::{Encode, SliceSink};

/// Bytes waiting to be written, in order, with the count already written.
#[derive(Debug)]
pub(crate) struct TxQueue<const N: usize> {
    buf: [u8; N],
    start: usize,
    end: usize,
    written: u64,
    queued: u64,
}

/// The message does not fit beside what is already queued.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Full;

impl<const N: usize> TxQueue<N> {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0; N],
            start: 0,
            end: 0,
            written: 0,
            queued: 0,
        }
    }

    pub(crate) fn pending(&self) -> &[u8] {
        self.buf.get(self.start..self.end).unwrap_or_default()
    }

    pub(crate) fn advance(&mut self, written: usize) {
        let written = written.min(self.pending().len());
        self.start = self.start.saturating_add(written);
        self.written = self
            .written
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
    }

    /// Queues `message`, returning the stream position its last byte will occupy.
    pub(crate) fn push(&mut self, message: &Message<'_>) -> Result<u64, Full> {
        let size = message.encoded_size().map_err(|_: MessageError| Full)?;
        if !self.has_room_for(size) {
            return Err(Full);
        }
        self.compact();
        let free = self.buf.get_mut(self.end..).ok_or(Full)?;
        let encoded = message
            .encode(&mut SliceSink::new(free))
            .map_err(|_| Full)?;
        self.end = self.end.saturating_add(encoded);
        self.queued = self
            .queued
            .saturating_add(u64::try_from(encoded).unwrap_or(u64::MAX));
        Ok(self.queued)
    }

    /// Whether a message of `size` encoded bytes fits beside what is queued.
    pub(crate) fn has_room_for(&self, size: usize) -> bool {
        size <= N.saturating_sub(self.pending().len())
    }

    /// Exchanges contents with `other`, of another capacity.
    ///
    /// # Errors
    ///
    /// [`DoesNotFit`] where either side holds more than the other can, leaving both
    /// holding what they did.
    pub(crate) fn swap<const M: usize>(
        &mut self,
        other: &mut TxQueue<M>,
    ) -> Result<(), DoesNotFit> {
        self.compact();
        other.compact();
        let span = self.end.max(other.end);
        let (Some(mine), Some(theirs)) =
            (self.buf.get_mut(..span), other.buf.get_mut(..span))
        else {
            return Err(DoesNotFit);
        };
        mine.swap_with_slice(theirs);
        core::mem::swap(&mut self.end, &mut other.end);
        core::mem::swap(&mut self.written, &mut other.written);
        core::mem::swap(&mut self.queued, &mut other.queued);
        Ok(())
    }

    fn compact(&mut self) {
        let len = self.pending().len();
        self.buf.copy_within(self.start..self.end, 0);
        self.start = 0;
        self.end = len;
    }

    pub(crate) fn written_through(&self, position: u64) -> bool {
        self.written >= position
    }

    pub(crate) fn clear(&mut self) {
        self.written = self.queued;
        self.start = 0;
        self.end = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalAddress;
    use crate::messages::ProtocolVersion;

    fn alive_check_response() -> Message<'static> {
        Message::alive_check_response(ProtocolVersion::V2019, LogicalAddress(0x0E00))
    }

    const ALIVE_CHECK_RESPONSE: [u8; 10] =
        [0x03, 0xFC, 0x00, 0x08, 0x00, 0x00, 0x00, 0x02, 0x0E, 0x00];

    #[test]
    fn a_pushed_message_is_pending_until_written() {
        let mut queue = TxQueue::<16>::new();
        let end = queue.push(&alive_check_response()).unwrap();
        assert_eq!(end, 10);
        assert_eq!(queue.pending(), ALIVE_CHECK_RESPONSE);
        assert!(!queue.written_through(end));

        queue.advance(3);
        assert_eq!(queue.pending(), &ALIVE_CHECK_RESPONSE[3..]);
        assert!(!queue.written_through(end));

        queue.advance(7);
        assert_eq!(queue.pending(), [0u8; 0]);
        assert!(queue.written_through(end));
    }

    #[test]
    fn positions_keep_counting_across_messages() {
        let mut queue = TxQueue::<16>::new();
        let first = queue.push(&alive_check_response()).unwrap();
        queue.advance(10);
        let second = queue.push(&alive_check_response()).unwrap();
        assert_eq!(second, first + 10);
        queue.advance(9);
        assert!(!queue.written_through(second));
        queue.advance(1);
        assert!(queue.written_through(second));
    }

    #[test]
    fn a_partly_written_message_moves_up_to_make_room() {
        let mut queue = TxQueue::<16>::new();
        queue.push(&alive_check_response()).unwrap();
        queue.advance(6);
        let end = queue.push(&alive_check_response()).unwrap();
        assert_eq!(end, 20);
        assert_eq!(queue.pending().len(), 14);
        assert_eq!(queue.pending().get(4..), Some(&ALIVE_CHECK_RESPONSE[..]));
    }

    #[test]
    fn a_message_that_does_not_fit_leaves_the_queue_unchanged() {
        let mut queue = TxQueue::<16>::new();
        queue.push(&alive_check_response()).unwrap();
        assert_eq!(queue.push(&alive_check_response()), Err(Full));
        assert_eq!(queue.pending(), ALIVE_CHECK_RESPONSE);
    }

    #[test]
    fn swapping_moves_what_is_pending_between_capacities() {
        let mut small = TxQueue::<16>::new();
        let mut large = TxQueue::<64>::new();
        large.push(&alive_check_response()).unwrap();
        large.advance(4);

        small.swap(&mut large).unwrap();

        assert_eq!(small.pending(), &ALIVE_CHECK_RESPONSE[4..]);
        assert_eq!(large.pending(), [0u8; 0]);
        assert!(small.has_room_for(10));
        assert!(!small.has_room_for(11));
    }

    #[test]
    fn swapping_more_than_fits_changes_nothing() {
        let mut small = TxQueue::<8>::new();
        let mut large = TxQueue::<64>::new();
        large.push(&alive_check_response()).unwrap();

        assert_eq!(small.swap(&mut large), Err(DoesNotFit));
        assert_eq!(large.pending(), ALIVE_CHECK_RESPONSE);
    }

    #[test]
    fn clearing_drops_what_was_pending() {
        let mut queue = TxQueue::<16>::new();
        queue.push(&alive_check_response()).unwrap();
        queue.advance(4);
        queue.clear();
        assert_eq!(queue.pending(), [0u8; 0]);
        let end = queue.push(&alive_check_response()).unwrap();
        assert_eq!(end, 20);
    }
}
