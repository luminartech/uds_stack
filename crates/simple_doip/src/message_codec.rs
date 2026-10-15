//! `tokio_util::codec` adapter that frames and decodes/encodes [`OwnedMessage`]
//! values directly on a byte stream, so `DoIP` connections can be driven with
//! `FramedRead`/`FramedWrite` instead of manual buffer management.

use crate::messages::{
    Decode, Encode, Header, Message, MessageError, OwnedMessage, Payload,
};
use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// What [`MessageCodec`] decodes from the stream: one frame's worth.
#[derive(Debug)]
#[non_exhaustive]
pub enum Frame {
    /// A message this crate models.
    Message(OwnedMessage),
    /// A frame whose header is sound but whose payload does not decode: a payload type
    /// this crate does not model, such as a vehicle-manufacturer-specific one, or a body
    /// its type rejects. The frame is consumed, so the stream stays in step.
    Undecoded {
        /// The frame's header.
        header: Header,
        /// The frame's payload, exactly [`Header::payload_length`] bytes.
        payload: Bytes,
        /// Why the payload did not decode; never
        /// [framing-fatal](MessageError::is_framing_fatal).
        error: MessageError,
    },
    /// A frame whose header declares a payload longer than
    /// [`MessageCodec::max_payload_length`]: ISO 13400-2:2019 Table 19's
    /// "message too large" (`0x02`). Its payload is discarded as it arrives, so the
    /// frame after it decodes.
    TooLarge {
        /// The frame's header.
        header: Header,
    },
}

/// Codec for the `DoIP` messages, used to encode and decode messages from
/// the TCP stream
#[derive(Debug)]
pub struct MessageCodec {
    max_payload_length: u32,
    /// Payload bytes of a [`Frame::TooLarge`] still to discard.
    discarding: u32,
}

impl Default for MessageCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageCodec {
    /// The default [`Self::max_payload_length`]: 8 MiB, as `tokio-util`'s
    /// `LengthDelimitedCodec` uses.
    pub const DEFAULT_MAX_PAYLOAD_LENGTH: u32 = 8 * 1024 * 1024;

    /// Create a codec that buffers payloads up to
    /// [`Self::DEFAULT_MAX_PAYLOAD_LENGTH`].
    #[must_use]
    pub const fn new() -> Self {
        Self::with_max_payload_length(Self::DEFAULT_MAX_PAYLOAD_LENGTH)
    }

    /// Create a codec that buffers payloads up to `max_payload_length` bytes and
    /// reports a longer one as [`Frame::TooLarge`].
    ///
    /// # Arguments
    ///
    /// * `max_payload_length` - the longest payload decoded, in bytes; `u32::MAX`
    ///   decodes every frame the header can describe.
    #[must_use]
    pub const fn with_max_payload_length(max_payload_length: u32) -> Self {
        Self {
            max_payload_length,
            discarding: 0,
        }
    }

    /// The longest payload this codec decodes, in bytes.
    #[must_use]
    pub const fn max_payload_length(&self) -> u32 {
        self.max_payload_length
    }
}

impl Decoder for MessageCodec {
    type Item = Frame;
    type Error = MessageError;

    /// Decode one `DoIP` frame from `src`.
    ///
    /// # Errors
    ///
    /// A [framing-fatal](MessageError::is_framing_fatal) [`MessageError`]: stream sync is
    /// lost and the connection must be closed. Every other decode failure is a
    /// [`Frame::Undecoded`].
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.discarding > 0 {
            let remaining = usize::try_from(self.discarding).unwrap_or(usize::MAX);
            let dropped = remaining.min(src.len());
            src.advance(dropped);
            let dropped = u32::try_from(dropped).unwrap_or(u32::MAX);
            self.discarding = self.discarding.saturating_sub(dropped);
            if self.discarding > 0 {
                return Ok(None);
            }
        }
        let Some(header_bytes) = src.get(..Header::SIZE) else {
            return Ok(None);
        };
        let (header, _) = Header::decode(header_bytes)?;
        if header.payload_length() > self.max_payload_length {
            src.advance(Header::SIZE);
            self.discarding = header.payload_length();
            return Ok(Some(Frame::TooLarge { header }));
        }
        let Some((frame, consumed)) = crate::try_frame(src.as_ref())? else {
            return Ok(None);
        };
        let decoded =
            Payload::decode(frame.payload, frame.header.payload_type()).map(|payload| {
                Message {
                    protocol_version: frame.header.protocol_version(),
                    payload,
                }
                .to_owned_message()
            });
        match decoded {
            Ok(owned) => {
                src.advance(consumed);
                Ok(Some(Frame::Message(owned)))
            }
            Err(error) if error.is_framing_fatal() => Err(error),
            Err(error) => {
                let header = frame.header.clone();
                let mut whole = src.split_to(consumed);
                let payload = whole.split_off(Header::SIZE).freeze();
                Ok(Some(Frame::Undecoded {
                    header,
                    payload,
                    error,
                }))
            }
        }
    }
}

impl Encoder<&OwnedMessage> for MessageCodec {
    type Error = MessageError;
    fn encode(
        &mut self,
        message: &OwnedMessage,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let size = message.encoded_size()?;
        // `automotive_wire_codec` ships no growable sink (see MIGRATION notes),
        // and the orphan rule blocks a local `Sink for Vec<u8>`/`BytesMut` impl
        // here. `size` is the exact byte count `encode` will write, so a slice
        // sized to it is bounded and exact, not actually open-ended.
        let mut out = std::vec![0u8; size];
        let mut sink = automotive_wire_codec::SliceSink::new(&mut out);
        message.encode(&mut sink)?;
        dst.reserve(size);
        dst.extend_from_slice(&out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{OwnedPayload, PayloadType};

    /// Well-formed NACK frame: 8-byte header (V2012, payload type 0x0000, length 1) plus
    /// a 1-byte body.
    const NACK_FRAME: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    /// Valid header, but payload type 0x9999 is unmodeled. Framing succeeds; the body
    /// decode fails with a recoverable `UnsupportedPayloadType`.
    const UNSUPPORTED_FRAME: [u8; 9] =
        [0x02, 0xFD, 0x99, 0x99, 0x00, 0x00, 0x00, 0x01, 0x07];

    /// Corrupt inverse protocol version (0xFE, expected 0xFD): framing-fatal.
    const CORRUPT_HEADER: [u8; 8] = [0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    /// A NACK header declaring a four-byte payload, and its four bytes.
    const FOUR_BYTE_FRAME: [u8; 12] = [
        0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xAA, 0xBB, 0xCC, 0xDD,
    ];

    /// Whether `frame` is a decoded generic header NACK.
    fn is_nack(frame: Option<&Frame>) -> bool {
        matches!(
            frame,
            Some(Frame::Message(OwnedMessage {
                payload: OwnedPayload::DoIPNack(_),
                ..
            }))
        )
    }

    #[test]
    fn partial_frame_returns_none_and_leaves_buffer_intact() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&NACK_FRAME[..4]);
        assert!(codec.decode(&mut src).unwrap().is_none());
        assert_eq!(src.len(), 4, "a partial frame must not be consumed");
    }

    #[test]
    fn two_frames_in_one_buffer_both_decode() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&NACK_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        assert!(is_nack(codec.decode(&mut src).unwrap().as_ref()));
        assert!(is_nack(codec.decode(&mut src).unwrap().as_ref()));
        assert!(src.is_empty(), "both frames should be consumed");
    }

    /// A payload type this crate does not model reaches the caller with its bytes, and
    /// the frame after it still decodes.
    #[test]
    fn an_unmodelled_payload_type_is_handed_over_undecoded() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&UNSUPPORTED_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        let Some(Frame::Undecoded {
            header,
            payload,
            error,
        }) = codec.decode(&mut src).unwrap()
        else {
            panic!("an unmodelled payload type must be handed over");
        };
        assert_eq!(header.payload_type(), PayloadType::from(0x9999));
        assert_eq!(&payload[..], &[0x07]);
        assert!(matches!(error, MessageError::UnsupportedPayloadType(_)));
        assert!(is_nack(codec.decode(&mut src).unwrap().as_ref()));
        assert!(src.is_empty());
    }

    /// A framing-fatal error still propagates: stream sync is lost and the connection
    /// must be torn down.
    #[test]
    fn corrupt_header_is_fatal() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&CORRUPT_HEADER[..]);
        let err = codec.decode(&mut src).unwrap_err();
        assert!(err.is_framing_fatal(), "got a non-fatal error: {err:?}");
        assert!(matches!(err, MessageError::VersionInverseIncorrect { .. }));
    }

    /// A `RoutingActivationRequest` (payload type `0x0005`) whose header declares a 1-byte
    /// body, and one body byte is present, so framing is satisfied, but Table 46 allows
    /// the request 7 or 11 bytes. It is `Payload::decode` that returns the framing-fatal
    /// `MessageError::InvalidPayloadLength`, which `decode` must propagate rather than
    /// hand over as a `Frame::Undecoded`.
    const TRUNCATED_BODY_FRAME: [u8; 9] =
        [0x02, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x01, 0x03];

    #[test]
    fn truncated_body_is_fatal_via_classifier() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&TRUNCATED_BODY_FRAME[..]);
        let err = codec.decode(&mut src).unwrap_err();
        assert!(err.is_framing_fatal(), "got a non-fatal error: {err:?}");
        assert!(matches!(
            err,
            MessageError::InvalidPayloadLength { length: 1, .. }
        ));
        assert_eq!(
            src.len(),
            TRUNCATED_BODY_FRAME.len(),
            "the buffer is left untouched"
        );
    }

    /// A payload past the limit is reported from its header alone, its bytes are
    /// discarded as they arrive, across reads, and the next frame decodes.
    #[test]
    fn a_payload_past_the_limit_is_reported_and_discarded() {
        let mut codec = MessageCodec::with_max_payload_length(3);
        let mut src = BytesMut::from(&FOUR_BYTE_FRAME[..10]);
        let Some(Frame::TooLarge { header }) = codec.decode(&mut src).unwrap() else {
            panic!("a payload past the limit must be reported");
        };
        assert_eq!(header.payload_length(), 4);
        assert!(codec.decode(&mut src).unwrap().is_none());
        assert!(src.is_empty(), "the payload's first bytes are discarded");

        src.extend_from_slice(&FOUR_BYTE_FRAME[10..]);
        src.extend_from_slice(&NACK_FRAME);
        assert!(is_nack(codec.decode(&mut src).unwrap().as_ref()));
        assert!(src.is_empty());
    }

    /// A payload at the limit decodes.
    #[test]
    fn a_payload_at_the_limit_is_buffered() {
        let mut codec = MessageCodec::with_max_payload_length(1);
        let mut src = BytesMut::from(&NACK_FRAME[..]);
        assert!(is_nack(codec.decode(&mut src).unwrap().as_ref()));
        assert_eq!(MessageCodec::new().max_payload_length(), 8 * 1024 * 1024);
    }
}
