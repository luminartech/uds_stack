//! What a slot sends, in two queues: the entity's own frames, and the diagnostic
//! messages the layer above requests.
//!
//! A response the size of the whole data queue still fits however many
//! acknowledgements, negative acknowledgements and alive check requests the socket
//! handler has queued ahead of it, because they queue apart. A frame once started is
//! written whole before the other queue is served, and between frames the entity's own
//! go first, so an acknowledgement still precedes the response to its request (ISO
//! 13400-2:2019 REQ 7.DoIP-067).

use crate::messages::Header;
use crate::stream::DoesNotFit;
use crate::stream::tx::TxQueue;

/// Room for the entity's own frames on one socket. The largest is a routing activation
/// response with its OEM-specific field; this holds a few frames of any kind.
pub(super) const CONTROL_CAP: usize = 64;

/// A slot's two transmit queues, and the frame being written from one of them.
pub(super) struct Outbox<const CAP: usize> {
    /// Acknowledgements, negative acknowledgements, routing activation responses and
    /// alive check requests.
    pub(super) control: TxQueue<CONTROL_CAP>,
    /// The diagnostic messages [`crate::service::DiagnosticEntity::request`] queues,
    /// whose stream positions their confirms wait on.
    pub(super) data: TxQueue<CAP>,
    /// The frame part written.
    writing: Option<Frame>,
}

/// A frame being written: the queue it is in, and how many of its bytes are left.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    lane: Lane,
    left: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Control,
    Data,
}

impl<const CAP: usize> Outbox<CAP> {
    pub(super) const fn new() -> Self {
        Self {
            control: TxQueue::new(),
            data: TxQueue::new(),
            writing: None,
        }
    }

    /// Whether nothing is queued in either.
    pub(super) fn is_empty(&self) -> bool {
        self.control.pending().is_empty() && self.data.pending().is_empty()
    }

    /// The bytes to write next, and which frame they are: the rest of the frame part
    /// written, or else the next whole frame, the entity's own first. Empty where
    /// nothing is queued. Nothing is decided until bytes are written, so a frame of the
    /// entity's queued while a write that wrote nothing was pending still goes first.
    pub(super) fn next(&self) -> (Frame, &[u8]) {
        let frame = self.writing.unwrap_or_else(|| {
            let lane = if self.control.pending().is_empty() {
                Lane::Data
            } else {
                Lane::Control
            };
            Frame {
                lane,
                left: frame_len(self.pending(lane)),
            }
        });
        let pending = self.pending(frame.lane);
        (frame, pending.get(..frame.left).unwrap_or(pending))
    }

    /// Records that `written` bytes of `frame`, as [`Self::next`] gave it, were written.
    pub(super) fn advance(&mut self, frame: Frame, written: usize) {
        if written == 0 {
            return;
        }
        match frame.lane {
            Lane::Control => self.control.advance(written),
            Lane::Data => self.data.advance(written),
        }
        let left = frame.left.saturating_sub(written);
        self.writing = (left > 0).then_some(Frame {
            lane: frame.lane,
            left,
        });
    }

    /// Drops everything queued.
    pub(super) fn clear(&mut self) {
        self.control.clear();
        self.data.clear();
        self.writing = None;
    }

    /// Exchanges contents with `other`, whose data queue has another capacity.
    ///
    /// # Errors
    ///
    /// [`DoesNotFit`] where either data queue holds more than the other can, leaving
    /// both holding what they did.
    pub(super) fn swap<const M: usize>(
        &mut self,
        other: &mut Outbox<M>,
    ) -> Result<(), DoesNotFit> {
        self.data.swap(&mut other.data)?;
        core::mem::swap(&mut self.control, &mut other.control);
        core::mem::swap(&mut self.writing, &mut other.writing);
        Ok(())
    }

    fn pending(&self, lane: Lane) -> &[u8] {
        match lane {
            Lane::Control => self.control.pending(),
            Lane::Data => self.data.pending(),
        }
    }
}

/// The length of the frame `pending` starts with, from its generic header's payload
/// length; all of `pending` where it is shorter than a header, which a queue of whole
/// frames never is.
fn frame_len(pending: &[u8]) -> usize {
    let payload = match pending.get(4..Header::SIZE) {
        Some(&[b0, b1, b2, b3]) => u32::from_be_bytes([b0, b1, b2, b3]),
        _ => return pending.len(),
    };
    usize::try_from(payload)
        .map_or(pending.len(), |payload| {
            Header::SIZE.saturating_add(payload)
        })
        .min(pending.len())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::LogicalAddress;
    use crate::messages::{Message, ProtocolVersion};
    use std::vec::Vec;

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);
    const ENTITY: LogicalAddress = LogicalAddress(0x0001);

    fn response(len: usize) -> Message<'static> {
        static DATA: [u8; 64] = [0x62; 64];
        Message::diagnostic_message(
            ProtocolVersion::V2019,
            ENTITY,
            TESTER,
            DATA.get(..len).unwrap_or_default(),
        )
    }

    fn ack() -> Message<'static> {
        Message::diagnostic_message_ack(ProtocolVersion::V2019, ENTITY, TESTER, &[])
    }

    fn drain<const CAP: usize>(outbox: &mut Outbox<CAP>, step: usize) -> Vec<u8> {
        let mut written = Vec::new();
        loop {
            let (frame, next) = outbox.next();
            if next.is_empty() {
                return written;
            }
            let take = next.len().min(step);
            written.extend_from_slice(next.get(..take).unwrap_or_default());
            outbox.advance(frame, take);
        }
    }

    fn encoded(message: &Message<'_>) -> Vec<u8> {
        let mut queue = TxQueue::<128>::new();
        queue.push(message).ok();
        queue.pending().to_vec()
    }

    /// A response filling the whole data queue is queued beside acknowledgements, and the
    /// acknowledgements go first.
    #[test]
    fn a_full_response_queues_beside_acknowledgements_which_go_first() {
        let mut outbox = Outbox::<32>::new();
        outbox.control.push(&ack()).ok();
        outbox.control.push(&ack()).ok();
        assert!(outbox.data.push(&response(20)).is_ok());

        let mut expected = encoded(&ack());
        expected.extend(encoded(&ack()));
        expected.extend(encoded(&response(20)));
        assert_eq!(drain(&mut outbox, 64), expected);
    }

    /// A frame part written is finished before the other queue is served, so a frame
    /// queued meanwhile never lands inside it.
    #[test]
    fn a_frame_part_written_is_finished_first() {
        let mut outbox = Outbox::<64>::new();
        outbox.data.push(&response(20)).ok();
        let (frame, first) = outbox.next();
        let first = first.len();
        outbox.advance(frame, 5);
        outbox.control.push(&ack()).ok();

        assert_eq!(first, encoded(&response(20)).len());
        let mut expected = encoded(&response(20)).split_off(5);
        expected.extend(encoded(&ack()));
        assert_eq!(drain(&mut outbox, 3), expected);
    }

    /// Frames from one queue are written one at a time, so the other queue's is served
    /// between them.
    #[test]
    fn an_own_frame_goes_between_two_responses() {
        let mut outbox = Outbox::<64>::new();
        outbox.data.push(&response(4)).ok();
        outbox.data.push(&response(4)).ok();
        let (frame, next) = outbox.next();
        assert_eq!(next.len(), encoded(&response(4)).len());
        outbox.control.push(&ack()).ok();
        outbox.advance(frame, encoded(&response(4)).len());

        let mut expected = encoded(&ack());
        expected.extend(encoded(&response(4)));
        assert_eq!(drain(&mut outbox, 64), expected);
    }
}
