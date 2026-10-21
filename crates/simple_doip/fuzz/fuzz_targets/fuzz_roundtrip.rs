#![no_main]

use automotive_wire_codec::SliceSink;
use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Decode, Encode, Message};

// The asymmetry hunt: if a frame decodes, re-encoding it and decoding that
// again must yield the same message.
//
//   decode(encode(decode(bytes))) == decode(bytes)
//
// A field written in one order and read in another survives a hand-written
// round-trip test whenever both sides share the mistake, but not this -- the
// re-encoded bytes have to be acceptable to the decoder that produced them.
fuzz_target!(|data: &[u8]| {
    let Ok((message, _rest)) = Message::decode(data) else {
        return;
    };

    let Ok(size) = message.encoded_size() else {
        return;
    };
    let mut encoded = vec![0u8; size];
    {
        let mut writer = SliceSink::new(&mut encoded);
        if message.encode(&mut writer).is_err() {
            return;
        }
    }

    let (reparsed, rest) =
        Message::decode(&encoded).expect("re-decoding an encoded message must not fail");
    assert!(rest.is_empty(), "re-encoding left trailing bytes");

    // A message is its protocol version and its payload: `encode` derives the
    // header from them, so nothing else can differ.
    assert_eq!(message, reparsed, "round trip changed the message");

    // Encoding is idempotent: having normalized once, a second pass must
    // produce the very same bytes. This is the property that catches a field
    // order asymmetry.
    let mut again = vec![0u8; reparsed.encoded_size().expect("size of a decoded message")];
    {
        let mut writer = SliceSink::new(&mut again);
        reparsed
            .encode(&mut writer)
            .expect("re-encode must not fail");
    }
    assert_eq!(encoded, again, "encoding is not idempotent");
});
