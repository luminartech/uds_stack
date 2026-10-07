use embassy_time::{Duration, Instant};

use crate::messages::{DiagnosticNackCode, NackCode};
use crate::service::DoIpResult;

/// ISO 13400-2:2019 Table 26 against 8.2.5.
pub(super) fn from_diagnostic_nack(code: DiagnosticNackCode) -> DoIpResult {
    match code {
        DiagnosticNackCode::InvalidSourceAddress => DoIpResult::InvalidSa,
        DiagnosticNackCode::UnknownTargetAddress => DoIpResult::UnknownTa,
        DiagnosticNackCode::DiagnosticMessageTooLarge => DoIpResult::MessageTooLarge,
        DiagnosticNackCode::OutOfMemory => DoIpResult::OutOfMemory,
        DiagnosticNackCode::TargetUnreachable => DoIpResult::TargetUnreachable,
        DiagnosticNackCode::UnknownNetwork
        | DiagnosticNackCode::TransportProtocolError
        | DiagnosticNackCode::Reserved(_) => DoIpResult::Error,
    }
}

/// ISO 13400-2:2019 Table 19 against 8.2.5.
pub(super) fn from_header_nack(code: NackCode) -> DoIpResult {
    match code {
        NackCode::IncorrectPatternFormat
        | NackCode::UnknownPayloadType
        | NackCode::InvalidPayloadLength => DoIpResult::HdrError,
        NackCode::MessageTooLarge => DoIpResult::MessageTooLarge,
        NackCode::OutOfMemory => DoIpResult::OutOfMemory,
        NackCode::Reserved(_) => DoIpResult::Error,
    }
}

/// `instant` as [`DiagnosticConnection::now`] reports it: milliseconds, truncated to
/// 32 bits.
///
/// [`DiagnosticConnection::now`]: crate::service::DiagnosticConnection::now
#[expect(
    clippy::cast_possible_truncation,
    reason = "the trait's clock is the milliseconds truncated to 32 bits"
)]
pub(super) fn millis(instant: Instant) -> u32 {
    instant.as_millis() as u32
}

/// The instant `deadline_ms` names on `embassy_time`'s clock, read at `now`: the
/// nearest instant whose milliseconds truncate to it, or `now` if it has passed.
pub(super) fn caller_deadline(deadline_ms: u32, now: Instant) -> Instant {
    let ahead = deadline_ms.wrapping_sub(millis(now)).cast_signed();
    match u64::try_from(ahead) {
        Ok(ahead) => after(now, Duration::from_millis(ahead)),
        Err(_) => now,
    }
}

/// `duration` after `start`, or the end of time if that is past it.
pub(super) fn after(start: Instant, duration: Duration) -> Instant {
    start.checked_add(duration).unwrap_or(Instant::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_diagnostic_nack_code_maps_to_its_doip_result() {
        for (code, result) in [
            (0x00, DoIpResult::Error),
            (0x01, DoIpResult::Error),
            (0x02, DoIpResult::InvalidSa),
            (0x03, DoIpResult::UnknownTa),
            (0x04, DoIpResult::MessageTooLarge),
            (0x05, DoIpResult::OutOfMemory),
            (0x06, DoIpResult::TargetUnreachable),
            (0x07, DoIpResult::Error),
            (0x08, DoIpResult::Error),
            (0x09, DoIpResult::Error),
            (0xFF, DoIpResult::Error),
        ] {
            assert_eq!(
                from_diagnostic_nack(DiagnosticNackCode::from(code)),
                result,
                "{code:#04X}"
            );
        }
    }

    #[test]
    fn each_header_nack_code_maps_to_its_doip_result() {
        for (code, result) in [
            (0x00, DoIpResult::HdrError),
            (0x01, DoIpResult::HdrError),
            (0x02, DoIpResult::MessageTooLarge),
            (0x03, DoIpResult::OutOfMemory),
            (0x04, DoIpResult::HdrError),
            (0x05, DoIpResult::Error),
            (0xFF, DoIpResult::Error),
        ] {
            assert_eq!(
                from_header_nack(NackCode::from(code)),
                result,
                "{code:#04X}"
            );
        }
    }

    #[test]
    fn a_deadline_ahead_is_that_many_milliseconds_ahead() {
        let now = Instant::from_millis(10_000);
        assert_eq!(
            caller_deadline(10_250, now),
            now + Duration::from_millis(250)
        );
    }

    #[test]
    fn a_deadline_passed_is_now() {
        let now = Instant::from_millis(10_000);
        assert_eq!(caller_deadline(9_000, now), now);
        assert_eq!(caller_deadline(10_000, now), now);
    }

    #[test]
    fn a_deadline_across_the_u32_wrap_is_still_ahead() {
        let now = Instant::from_millis(u64::from(u32::MAX) - 99);
        assert_eq!(caller_deadline(100, now), now + Duration::from_millis(200));
        let later = Instant::from_millis(u64::from(u32::MAX) + 1 + 50);
        assert_eq!(caller_deadline(u32::MAX - 49, later), later);
    }
}
