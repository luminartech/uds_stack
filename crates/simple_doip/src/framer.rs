//! Sans-io framer: extract complete `DoIP` frames from a byte buffer without owning
//! any I/O resource itself, and without interpreting the payload.

use crate::messages::{Decode, Header, MessageError};

/// A delimited, header-validated `DoIP` frame whose payload is not yet interpreted.
#[derive(Debug, PartialEq)]
pub struct RawFrame<'a> {
    /// The decoded 8-byte `DoIP` generic header (protocol version, its inverse,
    /// payload type, and payload length) that prefixed this frame on the wire.
    pub header: Header,
    /// The frame's payload bytes, exactly `header.payload_length()` long, not yet
    /// decoded into a concrete [`Payload`](crate::messages::Payload) variant.
    pub payload: &'a [u8],
}

/// Delimits one frame from the front of `buf`, validating its 8-byte header and leaving
/// its payload uninterpreted. `Ok(None)` means more bytes are needed.
///
/// The caller holds the frame's header, its payload bytes and the consumed count, so it
/// can NACK, ignore or skip the frame itself if
/// [`Payload::decode`](crate::messages::Payload::decode) then fails.
///
/// # Errors
///
/// A [`MessageError`] for a malformed header, such as an incorrect inverse protocol
/// version. Every error is [framing-fatal](MessageError::is_framing_fatal): stream sync
/// is lost and the connection should be closed.
///
/// # Examples
///
/// Frame and decode one message from a byte buffer, with no allocator:
///
/// ```
/// use simple_doip::{try_frame, messages::Payload};
///
/// // A complete DoIP NACK frame: 8-byte header + 1-byte body.
/// let buf = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
///
/// let (frame, consumed) = try_frame(&buf)?.expect("buffer holds a complete frame");
/// assert_eq!(consumed, 9);
///
/// let payload = Payload::decode(frame.payload, frame.header.payload_type())?;
/// assert!(matches!(payload, Payload::DoIPNack(_)));
/// # Ok::<(), simple_doip::messages::MessageError>(())
/// ```
pub fn try_frame(buf: &[u8]) -> Result<Option<(RawFrame<'_>, usize)>, MessageError> {
    if buf.len() < Header::SIZE {
        return Ok(None);
    }
    let (header, _) = Header::decode(&buf[..Header::SIZE])?;
    let payload_len = header.payload_length() as usize;
    // Compares the bytes available rather than computing `Header::SIZE + payload_len`,
    // which can overflow on a 32-bit target.
    if buf.len() - Header::SIZE < payload_len {
        return Ok(None);
    }
    let total = Header::SIZE + payload_len;
    Ok(Some((
        RawFrame {
            header,
            payload: &buf[Header::SIZE..total],
        },
        total,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{MessageError, Payload, PayloadType};

    #[test]
    fn incomplete_header_returns_none() {
        let buf = [0x02u8, 0xFD, 0x00];
        assert!(matches!(try_frame(&buf), Ok(None)));
    }

    #[test]
    fn complete_nack_frame_is_framed() {
        // 8-byte header + 1-byte NACK payload = 9 bytes total.
        let buf: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
        let (frame, consumed) = try_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, 9);
        assert_eq!(
            frame.header.payload_type(),
            crate::messages::PayloadType::NegativeAcknowledge
        );
        assert_eq!(frame.payload, &buf[8..9]);
    }

    /// A hostile `payload_length` of `u32::MAX` must yield `Ok(None)` (incomplete),
    /// not overflow `Header::SIZE + payload_len` on 32-bit targets.
    #[test]
    fn huge_payload_length_does_not_overflow() {
        let buf: [u8; 8] = [0x02, 0xFD, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(matches!(try_frame(&buf), Ok(None)));
    }

    #[test]
    fn corrupt_inverse_errors() {
        let buf: [u8; 8] = [0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(matches!(
            try_frame(&buf),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }

    /// `try_frame` delimits a frame whether or not `Payload::decode` then accepts its
    /// body: a frame of a payload type this crate does not model still frames, and its
    /// recoverable decode error leaves the caller the payload and the consumed count to
    /// skip it with.
    #[test]
    fn frame_then_decode_recoverability() {
        // (a) valid NACK frame: framing succeeds, payload decode succeeds.
        let buf: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
        let (frame, consumed) = try_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, 9);
        let payload = Payload::decode(frame.payload, frame.header.payload_type())
            .expect("a well-formed NACK payload should decode");
        assert!(matches!(payload, Payload::DoIPNack(_)));

        // (b) valid header, but payload_type = Reserved(0x9999): framing still
        // succeeds (header + length are all try_frame needs); payload decode fails
        // with UnsupportedPayloadType, but the client still holds frame.payload and
        // `consumed` to skip past the frame and resync.
        let buf: [u8; 9] = [0x02, 0xFD, 0x99, 0x99, 0x00, 0x00, 0x00, 0x01, 0x00];
        let (frame, consumed) = try_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, 9);
        assert_eq!(frame.payload, &buf[8..9]);
        let err = Payload::decode(frame.payload, frame.header.payload_type()).unwrap_err();
        assert!(matches!(
            err,
            MessageError::UnsupportedPayloadType(PayloadType::Reserved(0x9999))
        ));
        assert!(!err.is_framing_fatal());
    }
}
