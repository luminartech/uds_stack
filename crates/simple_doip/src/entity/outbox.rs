//! What a slot sends, in two queues: the entity's own frames, and the diagnostic
//! messages the layer above requests.
//!
//! A response the size of the whole data queue still fits however many
//! acknowledgements, negative acknowledgements and alive check requests the socket
//! handler has queued ahead of it, because they queue apart. A frame once started is
//! written whole before the other queue is served, and between frames the entity's own
//! go first, so an acknowledgement still precedes the response to its request (ISO
//! 13400-2:2019 REQ 7.DoIP-067).
//!
//! A write carries every whole frame queued in one queue, and a response queued while
//! nothing is being written and the data queue is empty takes the entity's own frames
//! queued ahead of it along, room permitting. An acknowledgement and the response to
//! its request so leave in one write. Two would cost a round trip: the backends cannot
//! disable Nagle's algorithm, so the second small write waits for the tester to
//! acknowledge the first, which a delayed acknowledgement holds back about 40 ms, most
//! of `P2_server`. A response within the entity's own frames' length of the data
//! queue's capacity still leaves in a second write.

use crate::messages::{Header, Message, MessageError};
use crate::stream::DoesNotFit;
use crate::stream::tx::{Full, TxQueue};
use crate::wire::Encode;

/// Room for the entity's own frames on one socket. The largest it sends is its routing
/// activation response, 17 bytes, as it never adds the OEM-specific field; this holds a
/// few frames of any kind.
pub(super) const CONTROL_CAP: usize = 64;

/// A slot's two transmit queues, and the frame being written from one of them.
pub(super) struct Outbox<const CAP: usize> {
    /// Acknowledgements, negative acknowledgements, routing activation responses and
    /// alive check requests.
    pub(super) control: TxQueue<CONTROL_CAP>,
    /// The diagnostic messages [`crate::service::DiagnosticEntity::request`] queues,
    /// whose stream positions their confirms wait on, and the entity's own frames that
    /// joined them.
    pub(super) data: TxQueue<CAP>,
    /// The rest of the frame part written.
    writing: Option<Frame>,
}

/// What [`Outbox::next`] gave to write: the queue it is in, how many bytes, and whether
/// they are the rest of a frame part written rather than whole frames.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    lane: Lane,
    left: usize,
    resuming: bool,
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

    /// Queues a diagnostic message, returning the stream position its last byte will
    /// occupy in the data queue. Where nothing is being written and the data queue is
    /// empty, the entity's own frames queued ahead of it move in front of it first, if
    /// both fit.
    pub(super) fn push(&mut self, message: &Message<'_>) -> Result<u64, Full> {
        let ahead = self.control.pending();
        let size = message.encoded_size().map_err(|_: MessageError| Full)?;
        if self.writing.is_none()
            && self.data.pending().is_empty()
            && !ahead.is_empty()
            && self.data.has_room_for(ahead.len().saturating_add(size))
        {
            let moved = ahead.len();
            self.data.push_encoded(ahead)?;
            self.control.advance(moved);
        }
        self.data.push(message)
    }

    /// The bytes to write next, and what they are: the rest of the frame part written,
    /// or else every whole frame in one queue, the entity's own first. Empty where
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
                left: self.pending(lane).len(),
                resuming: false,
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
        let left = if frame.resuming {
            frame.left.saturating_sub(written)
        } else {
            rest_of_frame(self.pending(frame.lane), written)
        };
        match frame.lane {
            Lane::Control => self.control.advance(written),
            Lane::Data => self.data.advance(written),
        }
        self.writing = (left > 0).then_some(Frame {
            lane: frame.lane,
            left,
            resuming: true,
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

/// How much of the frame byte `at` of `pending` falls in is left from there: zero where
/// `at` is a frame boundary. `pending` starts with a whole frame.
fn rest_of_frame(pending: &[u8], at: usize) -> usize {
    let mut end = 0_usize;
    while end < at {
        let len = frame_len(pending.get(end..).unwrap_or_default());
        if len == 0 {
            return 0;
        }
        end = end.saturating_add(len);
    }
    end.saturating_sub(at)
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
    use crate::messages::ProtocolVersion;
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

    /// Writes everything queued, at most `step` bytes a write, returning each write.
    fn drain<const CAP: usize>(outbox: &mut Outbox<CAP>, step: usize) -> Vec<Vec<u8>> {
        let mut writes = Vec::new();
        loop {
            let (frame, next) = outbox.next();
            if next.is_empty() {
                return writes;
            }
            let take = next.len().min(step);
            writes.push(next.get(..take).unwrap_or_default().to_vec());
            outbox.advance(frame, take);
        }
    }

    fn encoded(messages: &[Message<'_>]) -> Vec<u8> {
        let mut queue = TxQueue::<128>::new();
        for message in messages {
            queue.push(message).ok();
        }
        queue.pending().to_vec()
    }

    /// An acknowledgement and the response queued behind it leave in one write, and the
    /// response's position counts the acknowledgement's bytes moved in front of it.
    #[test]
    fn an_acknowledgement_and_its_response_leave_in_one_write() {
        let mut outbox = Outbox::<64>::new();
        outbox.control.push(&ack()).ok();
        let end = outbox.push(&response(4)).ok();

        assert_eq!(drain(&mut outbox, 64), [encoded(&[ack(), response(4)])]);
        let written = u64::try_from(encoded(&[ack(), response(4)]).len()).ok();
        assert_eq!(end, written);
        assert!(outbox.data.written_through(end.unwrap_or(u64::MAX)));
    }

    /// A response filling the whole data queue is queued beside acknowledgements, which
    /// go first, in a write of their own.
    #[test]
    fn a_full_response_queues_beside_acknowledgements_which_go_first() {
        let mut outbox = Outbox::<32>::new();
        outbox.control.push(&ack()).ok();
        outbox.control.push(&ack()).ok();
        assert!(outbox.push(&response(20)).is_ok());

        assert_eq!(
            drain(&mut outbox, 64),
            [encoded(&[ack(), ack()]), encoded(&[response(20)])]
        );
    }

    /// A response queued behind another the data queue holds stays behind the entity's
    /// own frames queued meanwhile, which go first between them.
    #[test]
    fn a_response_behind_another_leaves_the_entity_s_frames_in_their_queue() {
        let mut outbox = Outbox::<64>::new();
        outbox.push(&response(4)).ok();
        outbox.control.push(&ack()).ok();
        outbox.push(&response(4)).ok();

        assert_eq!(
            drain(&mut outbox, 64),
            [encoded(&[ack()]), encoded(&[response(4), response(4)])]
        );
    }

    /// A frame part written is finished before the other queue is served, so a frame
    /// queued meanwhile never lands inside it.
    #[test]
    fn a_frame_part_written_is_finished_first() {
        let mut outbox = Outbox::<64>::new();
        outbox.push(&response(20)).ok();
        let (frame, first) = outbox.next();
        assert_eq!(first, encoded(&[response(20)]));
        outbox.advance(frame, 5);
        outbox.control.push(&ack()).ok();

        let mut expected = encoded(&[response(20)]).split_off(5);
        expected.extend(encoded(&[ack()]));
        assert_eq!(drain(&mut outbox, 3).concat(), expected);
    }

    /// A write that stops inside the second of two frames finishes that frame, and the
    /// entity's frame queued meanwhile goes before the third.
    #[test]
    fn a_write_stopping_inside_a_later_frame_finishes_that_frame() {
        let mut outbox = Outbox::<128>::new();
        for _ in 0..3 {
            outbox.data.push(&response(4)).ok();
        }
        let one = encoded(&[response(4)]).len();
        let (frame, _) = outbox.next();
        outbox.advance(frame, one.saturating_add(2));
        outbox.control.push(&ack()).ok();

        let writes = drain(&mut outbox, 128);
        assert_eq!(
            writes,
            [
                encoded(&[response(4)]).split_off(2),
                encoded(&[ack()]),
                encoded(&[response(4)]),
            ]
        );
    }

    /// A write that stops on a frame boundary lets the entity's frame queued meanwhile
    /// go next.
    #[test]
    fn an_own_frame_goes_between_two_responses() {
        let mut outbox = Outbox::<64>::new();
        outbox.data.push(&response(4)).ok();
        outbox.data.push(&response(4)).ok();
        let (frame, next) = outbox.next();
        assert_eq!(next, encoded(&[response(4), response(4)]));
        outbox.control.push(&ack()).ok();
        outbox.advance(frame, encoded(&[response(4)]).len());

        assert_eq!(
            drain(&mut outbox, 64),
            [encoded(&[ack()]), encoded(&[response(4)])]
        );
    }
}
