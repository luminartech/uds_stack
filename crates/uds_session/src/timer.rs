//! One timer of ISO 14229-2:2021 9.6 Tables 7 and 8.
//!
//! ``UDSS_LLR_0075`` — running or not. ``UDSS_LLR_0076`` — loaded at start with the
//! parameter as it then stands. ``UDSS_LLR_0077`` — expires when the elapsed time reaches
//! the loaded value, or exceeds it where the requirement says so. ``UDSS_LLR_0078`` — only
//! a running timer expires. ``UDSS_LLR_0079`` — expiry is evaluated only when a timestamp
//! is supplied, which is why nothing here reads a clock. ``UDSS_LLR_0080`` — the deadline
//! is the first timestamp at which a supplied timestamp would expire the timer.
//! ``UDSS_LLR_0186`` — a reading may be taken a lead ahead of the expiry, without
//! changing what the timer was loaded with.

use core::marker::PhantomData;

use crate::time::Timestamp;

/// Which reading of ``UDSS_LLR_0077`` a timer takes, fixed by the requirement that runs it.
pub(crate) trait Rule: Copy + core::fmt::Debug + Eq {
    /// Whether expiry needs the elapsed time to exceed the loaded value strictly.
    const EXCEEDS: bool;
}

/// Expires when the elapsed time reaches the loaded value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reaches;

/// Expires only once the elapsed time strictly exceeds the loaded value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exceeds;

impl Rule for Reaches {
    const EXCEEDS: bool = false;
}

impl Rule for Exceeds {
    const EXCEEDS: bool = true;
}

/// A timer: stopped, or running since `start` loaded with `loaded` milliseconds, expiring
/// under the rule `R`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timer<R: Rule> {
    running: Option<Running>,
    rule: PhantomData<R>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Running {
    start: Timestamp,
    loaded: u32,
}

impl<R: Rule> Timer<R> {
    /// Not running. ``UDSS_LLR_0083`` and ``UDSS_LLR_0102`` start every timer here.
    pub(crate) const STOPPED: Self = Self {
        running: None,
        rule: PhantomData,
    };

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

    /// Whether a timestamp of `now` expires this timer (``UDSS_LLR_0077``).
    pub(crate) fn expired(&self, now: Timestamp) -> bool {
        self.expired_by(now, 0)
    }

    /// Whether a timestamp of `now` falls `lead` milliseconds or less before this timer's
    /// expiry: [`Timer::expired`] against the loaded value less `lead`, saturating at
    /// zero, so a `lead` not less than the loaded value is met at the start.
    ///
    /// ``UDSS_LLR_0117`` with ``UDSS_LLR_0186``. The timer stays loaded with the full
    /// value (``UDSS_LLR_0076``): the lead is applied where the timer is read, so the
    /// window the requirement names and the instant it is reported at stay distinct.
    pub(crate) fn expired_by(&self, now: Timestamp, lead: u32) -> bool {
        self.running.is_some_and(|r| {
            let elapsed = now.interval_since(r.start);
            let at = r.loaded.saturating_sub(lead);
            if R::EXCEEDS {
                elapsed > at
            } else {
                elapsed >= at
            }
        })
    }

    /// The loaded value less the time elapsed at `now`, or `None` while stopped
    /// (``UDSS_LLR_0172``).
    pub(crate) fn remaining(&self, now: Timestamp) -> Option<u32> {
        self.running
            .map(|r| r.loaded.saturating_sub(now.interval_since(r.start)))
    }

    /// The first timestamp at which a supplied timestamp would expire this timer
    /// (``UDSS_LLR_0080``), or `None` while stopped. For a `Timer<Exceeds>` that is one
    /// millisecond past the boundary: reporting the boundary itself makes a caller that
    /// wakes exactly then tick, find nothing, and spin until the clock advances.
    ///
    /// Only this report forms an instant. Expiry itself compares elapsed time with the
    /// loaded value, so it needs no addition that could overflow or misread the wrap.
    pub(crate) fn deadline(&self) -> Option<Timestamp> {
        self.deadline_by(0)
    }

    /// The first timestamp at which [`Timer::expired_by`] with the same `lead` would
    /// hold, or `None` while stopped: [`Timer::deadline`] moved `lead` milliseconds
    /// earlier, but never before the start.
    pub(crate) fn deadline_by(&self, lead: u32) -> Option<Timestamp> {
        self.running.map(|r| {
            let at = r.start.0.wrapping_add(r.loaded.saturating_sub(lead));
            Timestamp(at.wrapping_add(u32::from(R::EXCEEDS)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Exceeds, Reaches, Timer};
    use crate::time::Timestamp;

    /// ``UDSS_LLR_0075``, ``UDSS_LLR_0078`` — a stopped timer never expires.
    #[test]
    fn a_stopped_timer_never_expires() {
        let t = Timer::<Reaches>::STOPPED;
        assert!(!t.is_running());
        assert!(!t.expired(Timestamp(u32::MAX)));
        assert_eq!(t.deadline(), None);
    }

    /// ``UDSS_LLR_0077`` — "reaches" expires at the boundary, "exceeds" one later.
    #[test]
    fn reaches_and_exceeds_differ_by_one_millisecond() {
        let mut r = Timer::<Reaches>::STOPPED;
        let mut x = Timer::<Exceeds>::STOPPED;
        r.start(Timestamp(100), 50);
        x.start(Timestamp(100), 50);
        assert!(!r.expired(Timestamp(149)));
        assert!(r.expired(Timestamp(150)));
        assert!(!x.expired(Timestamp(150)));
        assert!(x.expired(Timestamp(151)));
        assert_eq!(r.deadline(), Some(Timestamp(150)));
        assert_eq!(x.deadline(), Some(Timestamp(151)));
    }

    /// #23 C42 — a timer's rule is part of its type, so its expiry and its deadline
    /// cannot be read under different rules: the deadline is the first instant `expired`
    /// holds, for both rules.
    #[test]
    fn the_deadline_is_the_first_instant_the_timer_is_expired() {
        let mut r = Timer::<Reaches>::STOPPED;
        let mut x = Timer::<Exceeds>::STOPPED;
        r.start(Timestamp(7), 50);
        x.start(Timestamp(7), 50);
        let before = |d: Timestamp| Timestamp(d.0.wrapping_sub(1));
        assert!(
            r.deadline()
                .is_some_and(|d| r.expired(d) && !r.expired(before(d)))
        );
        assert!(
            x.deadline()
                .is_some_and(|d| x.expired(d) && !x.expired(before(d)))
        );
    }

    /// ``UDSS_LLR_0172`` — what is left is the loaded value less the time elapsed.
    #[test]
    fn remaining_counts_down_to_zero() {
        let mut t = Timer::<Reaches>::STOPPED;
        assert_eq!(t.remaining(Timestamp(0)), None);
        t.start(Timestamp(u32::MAX - 9), 60);
        assert_eq!(t.remaining(Timestamp(u32::MAX - 9)), Some(60));
        assert_eq!(t.remaining(Timestamp(49)), Some(1));
        assert_eq!(t.remaining(Timestamp(100)), Some(0));
    }

    /// ``UDSS_LLR_0019`` — a timer started before the wrap expires after it.
    #[test]
    fn a_timer_runs_across_the_timestamp_wrap() {
        let mut t = Timer::<Reaches>::STOPPED;
        t.start(Timestamp(u32::MAX - 9), 20);
        assert!(!t.expired(Timestamp(9)));
        assert!(t.expired(Timestamp(10)));
        assert_eq!(t.deadline(), Some(Timestamp(10)));
    }

    /// ``UDSS_LLR_0076`` — the loaded value is the one at the start, not a later one.
    #[test]
    fn restarting_reloads_from_the_new_value() {
        let mut t = Timer::<Reaches>::STOPPED;
        t.start(Timestamp(0), 50);
        t.start(Timestamp(10), 5);
        assert_eq!(t.deadline(), Some(Timestamp(15)));
        assert!(t.expired(Timestamp(15)));
        t.stop();
        assert_eq!(t.deadline(), None);
    }

    /// ``UDSS_LLR_0186`` — a lead moves the expiry earlier by its own length, across the
    /// timestamp wrap (``UDSS_LLR_0019``), without changing the loaded value.
    #[test]
    fn a_lead_expires_the_timer_early_across_the_wrap() {
        let mut r = Timer::<Reaches>::STOPPED;
        let mut x = Timer::<Exceeds>::STOPPED;
        r.start(Timestamp(u32::MAX - 9), 50);
        x.start(Timestamp(u32::MAX - 9), 50);
        // Loaded 50 from MAX-9: expiry at 40; a lead of 10 brings it to 30.
        assert!(!r.expired_by(Timestamp(29), 10));
        assert!(r.expired_by(Timestamp(30), 10));
        assert!(!x.expired_by(Timestamp(30), 10));
        assert!(x.expired_by(Timestamp(31), 10));
        assert_eq!(r.deadline_by(10), Some(Timestamp(30)));
        assert_eq!(x.deadline_by(10), Some(Timestamp(31)));
        // A lead of zero is the plain timer.
        assert_eq!(r.deadline_by(0), r.deadline());
        assert!(!r.expired(Timestamp(39)));
        assert!(r.expired(Timestamp(40)));
    }

    /// ``UDSS_LLR_0186`` — a lead not less than the loaded value saturates: the timer is
    /// met at its start, never before it.
    #[test]
    fn a_lead_past_the_loaded_value_saturates_at_the_start() {
        let mut t = Timer::<Reaches>::STOPPED;
        t.start(Timestamp(u32::MAX), 50);
        assert!(t.expired_by(Timestamp(u32::MAX), 50));
        assert!(t.expired_by(Timestamp(u32::MAX), u32::MAX));
        assert_eq!(t.deadline_by(u32::MAX), Some(Timestamp(u32::MAX)));
        assert!(!Timer::<Reaches>::STOPPED.expired_by(Timestamp(0), 10));
        assert_eq!(Timer::<Reaches>::STOPPED.deadline_by(10), None);
    }
}
