use embassy_time::Instant;

use crate::LogicalAddress;
use crate::messages::{ActivationTypeCode, Header, Message, PayloadType};
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

    pub(super) fn pending(&self) -> &[u8] {
        &self.buf[self.written..self.len]
    }

    pub(super) fn advance(&mut self, written: usize) {
        self.written = self.len.min(self.written.saturating_add(written));
    }

    /// Whether none of the message has been written.
    pub(super) fn untouched(&self) -> bool {
        self.written == 0
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
        self.written = 0;
    }
}

/// The longest control message: a routing activation request without its OEM field.
const CONTROL: usize = Header::SIZE + 7;

/// A routing activation request or an alive check response being written, ahead of
/// [`Outgoing`], and when the last alive check response was written. Set only when
/// nothing is pending.
#[derive(Debug)]
pub(super) struct Control {
    buf: [u8; CONTROL],
    len: usize,
    written: usize,
    answering: bool,
    answered_at: Option<Instant>,
}

impl Control {
    pub(super) const fn new() -> Self {
        Self {
            buf: [0; CONTROL],
            len: 0,
            written: 0,
            answering: false,
            answered_at: None,
        }
    }

    /// ISO 13400-2:2019 Table 47: a default activation for `sa`, with no OEM field.
    pub(super) fn routing_activation_request(&mut self, sa: LogicalAddress) {
        let [high, low] = sa.0.to_be_bytes();
        let activation_type = u8::from(ActivationTypeCode::Default);
        self.set(
            PayloadType::RoutingActivationRequest,
            &[high, low, activation_type, 0, 0, 0, 0],
        );
        self.answering = false;
    }

    /// ISO 13400-2:2019 Table 28: the alive check response naming `sa`.
    pub(super) fn alive_check_response(&mut self, sa: LogicalAddress) {
        self.set(PayloadType::AliveCheckResponse, &sa.0.to_be_bytes());
        self.answering = true;
    }

    fn set(&mut self, payload_type: PayloadType, payload: &[u8]) {
        let version = u8::from(VERSION);
        let [type_high, type_low] = u16::from(payload_type).to_be_bytes();
        let length = u32::try_from(payload.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes();
        let header = [version, !version, type_high, type_low];
        let (head, rest) = self.buf.split_at_mut(header.len());
        head.copy_from_slice(&header);
        let (length_field, rest) = rest.split_at_mut(length.len());
        length_field.copy_from_slice(&length);
        let copied = payload.len().min(rest.len());
        rest[..copied].copy_from_slice(&payload[..copied]);
        self.len = Header::SIZE.saturating_add(copied);
        self.written = 0;
    }

    pub(super) fn pending(&self) -> &[u8] {
        &self.buf[self.written..self.len]
    }

    pub(super) fn advance(&mut self, written: usize) {
        self.written = self.len.min(self.written.saturating_add(written));
        if self.answering && self.written == self.len {
            self.answering = false;
            self.answered_at = Some(Instant::now());
        }
    }

    /// When the last alive check response on this connection was written in full.
    pub(super) fn answered_at(&self) -> Option<Instant> {
        self.answered_at
    }

    pub(super) fn clear(&mut self) {
        self.len = 0;
        self.written = 0;
        self.answering = false;
        self.answered_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::ProtocolVersion;

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);

    fn encoded<'b>(message: &Message<'_>, buf: &'b mut [u8]) -> &'b [u8] {
        let len = message.encode(&mut SliceSink::new(buf)).unwrap();
        &buf[..len]
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
