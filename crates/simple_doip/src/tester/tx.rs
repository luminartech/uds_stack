use embassy_time::Instant;

use crate::LogicalAddress;
use crate::messages::{ActivationTypeCode, Header, Message, RoutingActivationRequest};
use crate::wire::{Encode, SliceSink};

use super::VERSION;

/// The diagnostic message being written, and how much of it has been.
#[derive(Debug)]
pub(super) struct Outgoing<const N: usize> {
    buf: [u8; N],
    len: usize,
    written: usize,
}

/// The message does not fit in `N` bytes.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TooLarge;

impl<const N: usize> Outgoing<N> {
    pub(super) const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            written: 0,
        }
    }

    /// Replaces whatever was being written with `message`.
    ///
    /// # Errors
    ///
    /// [`TooLarge`] where `message` does not fit in `N` bytes; what was being written is
    /// kept.
    pub(super) fn load(&mut self, message: &Message<'_>) -> Result<(), TooLarge> {
        if message.encoded_size().map_err(|_| TooLarge)? > N {
            return Err(TooLarge);
        }
        let len = message
            .encode(&mut SliceSink::new(&mut self.buf))
            .map_err(|_| TooLarge)?;
        self.len = len;
        self.written = 0;
        Ok(())
    }

    /// The bytes of the message that are still to be written.
    pub(super) fn pending(&self) -> &[u8] {
        self.buf.get(self.written..self.len).unwrap_or_default()
    }

    /// Records that the first `written` bytes of [`Self::pending`] were written.
    pub(super) fn advance(&mut self, written: usize) {
        self.written = self.len.min(self.written.saturating_add(written));
    }

    /// Whether none of the message has been written.
    pub(super) fn untouched(&self) -> bool {
        self.written == 0
    }

    /// Drops the message, written or not.
    pub(super) fn clear(&mut self) {
        self.len = 0;
        self.written = 0;
    }
}

/// The longest control message: a routing activation request without its OEM field.
const CONTROL: usize = Header::SIZE + RoutingActivationRequest::PAYLOAD_SIZE;

/// A message the tester owes the entity outside any request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// ISO 13400-2:2019 Table 47: a default activation for the tester, with no OEM field.
    RoutingActivationRequest(LogicalAddress),
    /// ISO 13400-2:2019 Table 28: the alive check response naming the tester.
    AliveCheckResponse(LogicalAddress),
}

impl Owed {
    fn message(self) -> Message<'static> {
        match self {
            Self::RoutingActivationRequest(sa) => Message::routing_activation_request(
                VERSION,
                sa,
                ActivationTypeCode::Default,
                None,
            ),
            Self::AliveCheckResponse(sa) => Message::alive_check_response(VERSION, sa),
        }
    }
}

/// The routing activation request or alive check response being written, ahead of
/// [`Outgoing`], with the one owed next, and when the last alive check response was
/// written.
#[derive(Debug)]
pub(super) struct Control {
    buf: [u8; CONTROL],
    len: usize,
    written: usize,
    writing: Option<Owed>,
    next: Option<Owed>,
    answered_at: Option<Instant>,
}

impl Control {
    pub(super) const fn new() -> Self {
        Self {
            buf: [0; CONTROL],
            len: 0,
            written: 0,
            writing: None,
            next: None,
            answered_at: None,
        }
    }

    /// Owes a default routing activation request for `sa`.
    pub(super) fn routing_activation_request(&mut self, sa: LogicalAddress) {
        self.owe(Owed::RoutingActivationRequest(sa));
    }

    /// Owes an alive check response naming `sa`.
    pub(super) fn alive_check_response(&mut self, sa: LogicalAddress) {
        self.owe(Owed::AliveCheckResponse(sa));
    }

    /// Starts writing `owed` now if nothing is being written, or once what is has been.
    fn owe(&mut self, owed: Owed) {
        if self.writing.is_some() {
            self.next = Some(owed);
        } else {
            self.start(owed);
        }
    }

    fn start(&mut self, owed: Owed) {
        self.len = owed
            .message()
            .encode(&mut SliceSink::new(&mut self.buf))
            .unwrap_or(0);
        self.written = 0;
        self.writing = Some(owed);
    }

    /// The bytes of the message being written that are still to be written.
    pub(super) fn pending(&self) -> &[u8] {
        self.buf.get(self.written..self.len).unwrap_or_default()
    }

    /// Records that the first `written` bytes of [`Self::pending`] were written, and
    /// starts the message owed next once the one being written is done.
    pub(super) fn advance(&mut self, written: usize) {
        self.written = self.len.min(self.written.saturating_add(written));
        if self.written < self.len {
            return;
        }
        if let Some(Owed::AliveCheckResponse(_)) = self.writing.take() {
            self.answered_at = Some(Instant::now());
        }
        if let Some(next) = self.next.take() {
            self.start(next);
        }
    }

    /// When the last alive check response on this connection was written in full.
    pub(super) fn answered_at(&self) -> Option<Instant> {
        self.answered_at
    }

    /// Forgets everything owed and written, for a new connection.
    pub(super) fn clear(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::ProtocolVersion;

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);

    fn encoded<'b>(message: &Message<'_>, buf: &'b mut [u8]) -> &'b [u8] {
        let len = message.encode(&mut SliceSink::new(buf)).unwrap();
        buf.get(..len).unwrap()
    }

    #[test]
    fn the_control_messages_are_the_ones_the_message_types_encode() {
        let mut control = Control::new();

        control.routing_activation_request(TESTER);
        let mut buf = [0; 32];
        assert_eq!(
            control.pending(),
            encoded(
                &Message::routing_activation_request(
                    ProtocolVersion::V2019,
                    TESTER,
                    ActivationTypeCode::Default,
                    None,
                ),
                &mut buf
            )
        );
        control.advance(control.pending().len());

        control.alive_check_response(TESTER);
        assert_eq!(
            control.pending(),
            encoded(
                &Message::alive_check_response(ProtocolVersion::V2019, TESTER),
                &mut buf
            )
        );
    }

    #[test]
    fn a_message_is_pending_until_written() {
        let mut control = Control::new();
        control.alive_check_response(TESTER);
        let mut whole = [0; 10];
        whole.copy_from_slice(control.pending());

        control.advance(3);
        assert_eq!(control.pending(), &whole[3..]);
        control.advance(100);
        assert_eq!(control.pending(), [0u8; 0]);
    }

    /// Everything `control` has to write, written in pieces of `step` bytes.
    fn drained(control: &mut Control, step: usize, out: &mut [u8]) -> usize {
        let mut len = 0;
        loop {
            let pending = control.pending();
            if pending.is_empty() {
                return len;
            }
            let take = pending.len().min(step);
            let end = len.saturating_add(take);
            out.get_mut(len..end)
                .unwrap()
                .copy_from_slice(pending.get(..take).unwrap());
            len = end;
            control.advance(take);
        }
    }

    #[test]
    fn a_message_owed_while_another_is_being_written_follows_it_whole() {
        let mut control = Control::new();
        control.routing_activation_request(TESTER);
        let mut out = [0; 64];
        let first = control.pending().len().min(3);
        out.get_mut(..first)
            .unwrap()
            .copy_from_slice(control.pending().get(..first).unwrap());
        control.advance(first);

        control.alive_check_response(TESTER);
        let len =
            first.saturating_add(drained(&mut control, 5, out.get_mut(first..).unwrap()));

        let mut expected = [0; 64];
        let activation = encoded(
            &Message::routing_activation_request(
                ProtocolVersion::V2019,
                TESTER,
                ActivationTypeCode::Default,
                None,
            ),
            &mut expected,
        )
        .len();
        let (_, rest) = expected.split_at_mut(activation);
        let alive = encoded(
            &Message::alive_check_response(ProtocolVersion::V2019, TESTER),
            rest,
        )
        .len();
        assert_eq!(
            out.get(..len),
            expected.get(..activation.saturating_add(alive))
        );
    }

    #[test]
    fn an_alive_check_response_is_answered_once_its_last_byte_is_written() {
        let mut control = Control::new();
        control.routing_activation_request(TESTER);
        control.advance(1);
        control.alive_check_response(TESTER);
        let mut out = [0; 64];

        control.advance(control.pending().len());
        assert_eq!(
            control.answered_at(),
            None,
            "only the activation request is written"
        );
        drained(&mut control, usize::MAX, &mut out);
        assert!(control.answered_at().is_some());
    }

    #[test]
    fn a_message_larger_than_n_is_not_loaded() {
        let mut outgoing = Outgoing::<16>::new();
        let fits =
            Message::diagnostic_message(ProtocolVersion::V2019, TESTER, TESTER, &[1; 4]);
        let too_large =
            Message::diagnostic_message(ProtocolVersion::V2019, TESTER, TESTER, &[1; 5]);

        assert_eq!(outgoing.load(&too_large), Err(TooLarge));
        assert_eq!(outgoing.pending(), [0u8; 0]);
        outgoing.load(&fits).unwrap();
        let mut buf = [0; 16];
        assert_eq!(outgoing.pending(), encoded(&fits, &mut buf));
    }

    #[test]
    fn clearing_drops_what_was_pending() {
        let mut outgoing = Outgoing::<16>::new();
        let message =
            Message::diagnostic_message(ProtocolVersion::V2019, TESTER, TESTER, &[1]);
        outgoing.load(&message).unwrap();
        outgoing.advance(4);
        outgoing.clear();
        assert_eq!(outgoing.pending(), [0u8; 0]);
    }
}
