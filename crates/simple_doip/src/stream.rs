//! The byte streams under a `DoIP` TCP connection, and their clock, shared by the tester
//! and the entity.

use embassy_time::{Duration, Instant};

pub(crate) mod rx;
pub(crate) mod tx;

/// `instant` as [`DiagnosticConnection::now`] reports it: milliseconds, truncated to
/// 32 bits.
///
/// [`DiagnosticConnection::now`]: crate::service::DiagnosticConnection::now
pub(crate) fn millis(instant: Instant) -> u32 {
    let [b0, b1, b2, b3, ..] = instant.as_millis().to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// Copies as much of `data` into `buf` as fits, returning how much that was.
pub(crate) fn copy(data: &[u8], buf: &mut [u8]) -> usize {
    let copied = data.len().min(buf.len());
    if let (Some(to), Some(from)) = (buf.get_mut(..copied), data.get(..copied)) {
        to.copy_from_slice(from);
    }
    copied
}

/// Contents too long for the buffer they were to move into.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DoesNotFit;

/// The instant `deadline_ms` names on `embassy_time`'s clock, read at `now`: the
/// nearest instant whose milliseconds truncate to it, or `now` if it has passed.
pub(crate) fn caller_deadline(deadline_ms: u32, now: Instant) -> Instant {
    let ahead = deadline_ms.wrapping_sub(millis(now)).cast_signed();
    match u64::try_from(ahead) {
        Ok(ahead) => after(now, Duration::from_millis(ahead)),
        Err(_) => now,
    }
}

/// `duration` after `start`, or the end of time if that is past it.
pub(crate) fn after(start: Instant, duration: Duration) -> Instant {
    start.checked_add(duration).unwrap_or(Instant::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
