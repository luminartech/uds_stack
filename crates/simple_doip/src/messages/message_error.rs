//! Error taxonomy for the `no_std` decode/encode core.
//!
//! [`MessageError`]'s variants fall along two axes: the framing tier, and the layer
//! that produces them.
//!
//! | Variant | Framing tier (per [`MessageError::is_framing_fatal`]) | Layer |
//! |---|---|---|
//! | `VersionInverseIncorrect` | framing-fatal | wire decode |
//! | `Incomplete` | framing-fatal | wire decode |
//! | `InvalidPayloadLength` | framing-fatal (Table 19 closes the socket) | wire decode |
//! | `TrailingBytes` | recoverable | wire decode |
//! | `UnsupportedPayloadType` | recoverable (body) | wire decode |
//! | `PayloadTooLarge` | recoverable | encode |
//! | `Io` | recoverable (TX-side short write, e.g. `Io(WriteError::Insufficient(..))` on an undersized stack buffer) | encode / automotive-wire-codec |
//! | `Std` | not a frame property at all | tokio/codec boundary |

use crate::messages::header::PayloadType;

use automotive_wire_codec::{Incomplete, TrailingBytes};
use thiserror::Error;

/// Wire-level encode/decode failures from the `no_std` `DoIP` message core.
///
/// See the module-level table above for how each variant classifies along the
/// framing-fatal/recoverable axis (via [`MessageError::is_framing_fatal`]) and the
/// wire-decode/encode/transport layer axis.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MessageError {
    /// The header's inverse protocol version byte did not match
    /// the bitwise complement of the protocol version byte. Since these two bytes
    /// are how a `DoIP` receiver validates frame sync, this is framing-fatal: the
    /// connection should be closed rather than the frame skipped.
    #[error("Version Inverse Incorrect, Expected: {expected:X}, got: {value:X}")]
    VersionInverseIncorrect {
        /// The inverse protocol version that should have been present, computed
        /// from the header's protocol version byte.
        expected: u8,
        /// The inverse protocol version byte actually found in the header.
        value: u8,
    },
    /// A payload's length is not one its payload type allows
    /// ([`PayloadType::allows_payload_length`]), whether too short, too long, or
    /// between the lengths a type with an optional field allows. An entity answers it
    /// with generic header NACK `0x04` and closes the socket (Table 19), so
    /// [`MessageError::is_framing_fatal`] classifies it fatal.
    #[error("Invalid payload length {length} for payload type {payload_type:?}")]
    InvalidPayloadLength {
        /// The payload type the header declared.
        payload_type: PayloadType,
        /// The payload's length in bytes.
        length: usize,
    },
    /// The payload encodes to more bytes than the header's `payload_length`
    /// field can describe. `DoIP` declares that length as a `u32`, so such a
    /// message has no valid wire form. Recoverable: nothing was written.
    ///
    /// Unreachable for a frame that came off the wire, whose length was itself
    /// a `u32`; it takes a hand-built [`Payload`](crate::messages::Payload)
    /// borrowing more than `u32::MAX` bytes.
    #[error("Payload of {size} bytes exceeds the u32 payload_length field")]
    PayloadTooLarge {
        /// The payload's encoded size.
        size: usize,
    },
    /// The header declared a [`PayloadType`] this crate does not implement a
    /// decoder for. Recoverable: the frame's header and payload bytes are still
    /// available (via [`crate::try_frame`]) so the caller can skip/resync.
    #[error("Unsupported payload type, cannot decode: {0:?}")]
    UnsupportedPayloadType(PayloadType),
    /// `buf` did not contain enough bytes to decode the requested item.
    ///
    /// Always classified framing-fatal by [`MessageError::is_framing_fatal`].
    /// [`crate::try_frame`] never returns this variant: a buffer too short to hold
    /// a complete frame yields `Ok(None)` so the caller can wait for more bytes.
    /// [`Payload::decode`](crate::messages::Payload::decode) does not either: a
    /// payload too short for its type is [`MessageError::InvalidPayloadLength`].
    /// `Incomplete` arises from a [`Decode`](crate::messages::Decode) call on a
    /// buffer shorter than the item, such as a message whose header claims more
    /// bytes than follow it.
    #[error(transparent)]
    Incomplete(#[from] Incomplete),
    /// `buf` contained more bytes than the item being decoded consumed, from
    /// [`Decode::decode_exact`](crate::messages::Decode::decode_exact). Recoverable:
    /// the item itself decoded.
    #[error(transparent)]
    TrailingBytes(#[from] TrailingBytes),
    /// A [`Sink`](automotive_wire_codec::Sink) write failed while encoding a message (e.g.
    /// [`WriteError::Insufficient`](automotive_wire_codec::WriteError::Insufficient) from
    /// an undersized stack buffer). Recoverable on the TX side: the caller can retry with a
    /// larger buffer.
    #[error(transparent)]
    Io(#[from] automotive_wire_codec::WriteError),
    /// Full `std::io::Error` from the tokio/codec layer, preserving the OS error detail
    /// (code and message) that the flattened [`MessageError::Io`] kind would lose. The
    /// `no_std` core never produces this variant.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Std(std::io::Error),
}

impl MessageError {
    /// Framing-fatal errors leave closing the connection as the only safe move:
    /// stream sync is lost (the header cannot be trusted), or the payload has a
    /// length its type does not allow, for which Table 19 has the socket closed.
    /// Everything else is frame-recoverable (NACK / ignore / skip).
    #[must_use]
    pub fn is_framing_fatal(&self) -> bool {
        matches!(
            self,
            MessageError::VersionInverseIncorrect { .. }
                | MessageError::Incomplete(_)
                | MessageError::InvalidPayloadLength { .. }
        )
    }
}

/// `tokio_util::codec::Decoder` requires its `Error` to be `From<std::io::Error>`. The
/// error is kept whole, as [`MessageError::Std`].
#[cfg(feature = "std")]
impl From<std::io::Error> for MessageError {
    fn from(err: std::io::Error) -> Self {
        MessageError::Std(err)
    }
}
