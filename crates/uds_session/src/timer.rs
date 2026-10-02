//! One timer of ISO 14229-2:2021 9.6 Tables 7 and 8.
//!
//! ``UDSS_LLR_0075`` — running or not. ``UDSS_LLR_0076`` — loaded at start with the
//! parameter as it then stands. ``UDSS_LLR_0077`` — expires when the elapsed time reaches
//! the loaded value, or exceeds it where the requirement says so. ``UDSS_LLR_0078`` — only
//! a running timer expires. ``UDSS_LLR_0079`` — expiry is evaluated only when a timestamp
//! is supplied, which is why nothing here reads a clock. ``UDSS_LLR_0080`` — the deadline
//! is the first timestamp at which a supplied timestamp would expire the timer.

use crate::time::Timestamp;

/// Which reading of ``UDSS_LLR_0077`` a timer takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expiry {
    /// Expires when the elapsed time reaches the loaded value.
    Reaches,
    /// Expires only once the elapsed time strictly exceeds the loaded value.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no server timer expires on exceeds; tP_Client of UDSS_LLR_0148 does"
        )
    )]
    Exceeds,
}

/// A timer: stopped, or running since `start` loaded with `loaded` milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timer {
    running: Option<Running>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Running {
    start: Timestamp,
    loaded: u32,
}

impl Timer {
    /// Not running. ``UDSS_LLR_0083`` and ``UDSS_LLR_0102`` start every timer here.
    pub(crate) const STOPPED: Self = Self { running: None };

    /// Set running at `now`, loaded with `loaded` (``UDSS_LLR_0076``). A running timer is
    /// restarted: the standard's "restart" is this call.
    pub(crate) fn start(&mut self, now: Timestamp, loaded: u32) {
        self.running = Some(Running { start: now, loaded });
    }

    /// Stop. Nothing is remembered (``UDSS_LLR_0078``).
    pub(crate) fn stop(&mut self) {
        self.running = None;
    }

    pub(crate) const fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The value loaded at the last start, while running (``UDSS_LLR_0101`` reads it).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no milestone-1 caller; the server keeps its reload"
        )
    )]
    pub(crate) fn loaded(&self) -> Option<u32> {
        self.running.map(|r| r.loaded)
    }

    /// Whether a timestamp of `now` expires this timer (``UDSS_LLR_0077``).
    pub(crate) fn expired(&self, now: Timestamp, rule: Expiry) -> bool {
        self.running.is_some_and(|r| {
            let elapsed = now.interval_since(r.start);
            match rule {
                Expiry::Reaches => elapsed >= r.loaded,
                Expiry::Exceeds => elapsed > r.loaded,
            }
        })
    }

    /// The first timestamp at which a supplied timestamp would expire this timer
    /// (``UDSS_LLR_0080``), or `None` while stopped. For [`Expiry::Exceeds`] that is one
    /// millisecond past the boundary: reporting the boundary itself makes a caller that
    /// wakes exactly then tick, find nothing, and spin until the clock advances.
    pub(crate) fn deadline(&self, rule: Expiry) -> Option<Timestamp> {
        self.running.map(|r| {
            let at = r.start.0.wrapping_add(r.loaded);
            Timestamp(match rule {
                Expiry::Reaches => at,
                Expiry::Exceeds => at.wrapping_add(1),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Expiry, Timer};
    use crate::time::Timestamp;

    /// ``UDSS_LLR_0075``, ``UDSS_LLR_0078`` — a stopped timer never expires.
    #[test]
    fn a_stopped_timer_never_expires() {
        let t = Timer::STOPPED;
        assert!(!t.is_running());
        assert!(!t.expired(Timestamp(u32::MAX), Expiry::Reaches));
        assert_eq!(t.deadline(Expiry::Reaches), None);
    }

    /// ``UDSS_LLR_0077`` — "reaches" expires at the boundary, "exceeds" one later.
    #[test]
    fn reaches_and_exceeds_differ_by_one_millisecond() {
        let mut t = Timer::STOPPED;
        t.start(Timestamp(100), 50);
        assert!(!t.expired(Timestamp(149), Expiry::Reaches));
        assert!(t.expired(Timestamp(150), Expiry::Reaches));
        assert!(!t.expired(Timestamp(150), Expiry::Exceeds));
        assert!(t.expired(Timestamp(151), Expiry::Exceeds));
        assert_eq!(t.deadline(Expiry::Reaches), Some(Timestamp(150)));
        assert_eq!(t.deadline(Expiry::Exceeds), Some(Timestamp(151)));
    }

    /// ``UDSS_LLR_0019`` — a timer started before the wrap expires after it.
    #[test]
    fn a_timer_runs_across_the_timestamp_wrap() {
        let mut t = Timer::STOPPED;
        t.start(Timestamp(u32::MAX - 9), 20);
        assert!(!t.expired(Timestamp(9), Expiry::Reaches));
        assert!(t.expired(Timestamp(10), Expiry::Reaches));
        assert_eq!(t.deadline(Expiry::Reaches), Some(Timestamp(10)));
    }

    /// ``UDSS_LLR_0076`` — the loaded value is the one at the start, not a later one.
    #[test]
    fn restarting_reloads_from_the_new_value() {
        let mut t = Timer::STOPPED;
        t.start(Timestamp(0), 50);
        t.start(Timestamp(10), 5);
        assert_eq!(t.loaded(), Some(5));
        assert!(t.expired(Timestamp(15), Expiry::Reaches));
        t.stop();
        assert_eq!(t.loaded(), None);
    }
}
