// Fuzz the Response decode path with arbitrary bytes.
// Any panic here is a bug: decode returns Err rather than crashing.
#![no_main]
use libfuzzer_sys::fuzz_target;
use uds_protocol::{Decode, Response};

fuzz_target!(|data: &[u8]| {
    // Attempt to decode arbitrary bytes as a UDS response. The result is irrelevant; the
    // target checks only that decoding does not panic.
    let _ = Response::decode(data);
});
