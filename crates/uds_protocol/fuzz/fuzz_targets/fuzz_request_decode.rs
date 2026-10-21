// Fuzz the Request decode path with arbitrary bytes.
// Any panic here is a bug: decode returns Err rather than crashing.
#![no_main]
use libfuzzer_sys::fuzz_target;
use uds_protocol::{Decode, Request};

fuzz_target!(|data: &[u8]| {
    // Attempt to decode arbitrary bytes as a UDS request. The result is irrelevant; the
    // target checks only that decoding does not panic.
    // Reporting SPRMIB indexes into the payload of an unmodeled service, so run it on every
    // frame that decodes.
    if let Ok((request, _)) = Request::decode(data) {
        let _ = request.is_positive_response_suppressed();
    }
});
