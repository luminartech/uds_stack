use crate::messages::{Message, MessageError};
use crate::wire::{Encode, SliceSink};

/// Bytes waiting to be written, in order, with the count already written.
#[derive(Debug)]
pub(super) struct TxQueue<const N: usize> {
    buf: [u8; N],
    start: usize,
    end: usize,
    written: u64,
    queued: u64,
}

/// The message does not fit beside what is already queued.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Full;

impl<const N: usize> TxQueue<N> {
    pub(super) const fn new() -> Self {
        Self {
            buf: [0; N],
            start: 0,
            end: 0,
            written: 0,
            queued: 0,
        }
    }

    pub(super) fn pending(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    pub(super) fn advance(&mut self, written: usize) {
        let written = written.min(self.end - self.start);
        self.start += written;
        self.written += written as u64;
    }

    /// Queues `message`, returning the stream position its last byte will occupy.
    pub(super) fn push(&mut self, message: &Message<'_>) -> Result<u64, Full> {
        let size = message.encoded_size().map_err(|_: MessageError| Full)?;
        if size > N - (self.end - self.start) {
            return Err(Full);
        }
        self.buf.copy_within(self.start..self.end, 0);
        self.end -= self.start;
        self.start = 0;
        let encoded = message
            .encode(&mut SliceSink::new(&mut self.buf[self.end..]))
            .map_err(|_| Full)?;
        self.end += encoded;
        self.queued += encoded as u64;
        Ok(self.queued)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the confirm timer that uses it lands in a later commit"
        )
    )]
    pub(super) fn written_through(&self, position: u64) -> bool {
        self.written >= position
    }

    pub(super) fn clear(&mut self) {
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
