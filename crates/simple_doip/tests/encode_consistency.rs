//! `Message::encode` must never emit a frame `Message::decode` rejects, and
//! `Message::decode` must reject a frame whose declared length its payload does not
//! occupy exactly.
//!
//! Regression tests for the asymmetry the `fuzz_roundtrip` target found: a decoded
//! `Message` once carried a declared length its payload did not occupy. `decode` now
//! refuses such a frame with `InvalidPayloadLength` (REQ 7.DoIP-045), and `encode`
//! derives the length from the payload.

// Test code. `unwrap`, `expect` and `panic` are the failure mechanism here, and a
// test that indexes a known-length golden vector reads better than one that does
// not. The workspace lint standard in the root Cargo.toml reaches this target too --
// an integration test is its own crate, so lib.rs cannot speak for it -- which is why
// the relaxation is stated here rather than left implicit.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use automotive_wire_codec::SliceSink;
use simple_doip::messages::{Decode, Encode, Message, MessageError, PayloadType};

/// Encode `message` into a fresh buffer sized by `encoded_size`.
fn encode(message: &Message<'_>) -> Vec<u8> {
    let mut buf = vec![0u8; message.encoded_size().expect("encoded_size failed")];
    {
        let mut writer = SliceSink::new(&mut buf);
        message.encode(&mut writer).expect("encode failed");
    }
    buf
}

/// A NACK body is one byte. This frame's header claims five.
#[test]
fn a_fixed_size_payload_with_an_overlong_declared_length_is_refused() {
    let framed: [u8; 13] = [
        0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x03, 0x00, 0x00, 0x00, 0x00,
    ];
    assert!(matches!(
        Message::decode(&framed),
        Err(MessageError::InvalidPayloadLength {
            payload_type: PayloadType::NegativeAcknowledge,
            length: 5,
        })
    ));
}

/// `VehicleIdentificationRequest` carries no data. This frame's header claims one
/// byte.
#[test]
fn a_unit_payload_with_a_nonzero_declared_length_is_refused() {
    let framed: [u8; 9] = [0x00, 0xFF, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00];
    assert!(matches!(
        Message::decode(&framed),
        Err(MessageError::InvalidPayloadLength {
            payload_type: PayloadType::VehicleIdentificationRequest,
            length: 1,
        })
    ));
}

/// A well-formed frame is untouched: the derived length equals the declared
/// one, so the bytes are identical. This is what keeps the golden vectors
/// valid.
#[test]
fn a_well_formed_frame_encodes_to_the_bytes_it_came_from() {
    let framed: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
    let (message, rest) = Message::decode(&framed).expect("decode");
    assert_eq!(rest, []);
    assert_eq!(encode(&message), framed);
}
