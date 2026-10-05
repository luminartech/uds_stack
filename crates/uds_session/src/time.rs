//! The timebase: what a timestamp is, and how an interval is computed from two.
//!
//! ``UDSS_LLR_0017`` forbids the session layer to read a clock, so every value here
//! originates with the caller.

/// A point in time, as the caller reports it.
///
/// ``UDSS_LLR_0018`` — a 32-bit unsigned count of milliseconds. ``UDSS_LLR_0020``
/// accompanies every input with one.
///
/// The derived `Ord` compares the raw counts, which is not the order in time once the
/// count wraps, every 2^32 ms (about 49.7 days). Compare against a deadline with
/// [`Self::has_reached`], and turn one into a wait with [`Self::until`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u32);

impl Timestamp {
    /// The elapsed time since `earlier`, in milliseconds.
    ///
    /// ``UDSS_LLR_0019`` — the difference modulo 2^32, so a timebase that wraps yields
    /// the right interval without the caller having to notice the wrap.
    #[must_use]
    pub const fn interval_since(self, earlier: Self) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }

    /// Whether `self`, read as the current time, has reached `deadline`.
    ///
    /// ``UDSS_LLR_0019`` — on the wrapping timebase, `deadline` has been reached when the
    /// interval since it is less than half the range (2^31 ms), and is still ahead
    /// otherwise. This is how a transport decides that the deadline the driver handed it
    /// has passed: `now >= deadline` through the derived `Ord` gets the answer wrong
    /// whenever the two lie either side of the wrap.
    ///
    /// # Arguments
    ///
    /// * `deadline` - the [`Timestamp`] to compare `self` against
    #[must_use]
    pub const fn has_reached(self, deadline: Self) -> bool {
        self.interval_since(deadline) <= u32::MAX / 2
    }

    /// How long from `self` until `deadline`, in milliseconds: zero once
    /// [`Self::has_reached`] says it has been reached, so a deadline already in the past
    /// never turns into a wait of nearly 2^32 ms.
    ///
    /// # Arguments
    ///
    /// * `deadline` - the [`Timestamp`] to wait for
    #[must_use]
    pub const fn until(self, deadline: Self) -> u32 {
        if self.has_reached(deadline) {
            0
        } else {
            deadline.interval_since(self)
        }
    }
}

/// The earlier of two wrapping timestamps (``UDSS_LLR_0019``): `a` is earlier when the
/// modular difference `a - b` lands in the upper half of the range.
pub(crate) const fn earlier(a: Timestamp, b: Timestamp) -> Timestamp {
    if a.0.wrapping_sub(b.0) > u32::MAX / 2 {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::{Timestamp, earlier};

    /// ``UDSS_LLR_0019`` — the earlier deadline is chosen by modular distance, so one
    /// just past the wrap is later than one just before it.
    #[test]
    fn the_earlier_deadline_is_chosen_across_the_wrap() {
        assert_eq!(earlier(Timestamp(10), Timestamp(20)), Timestamp(10));
        assert_eq!(earlier(Timestamp(20), Timestamp(10)), Timestamp(10));
        let before = Timestamp(u32::MAX - 5);
        let after = Timestamp(5);
        assert_eq!(earlier(before, after), before);
        assert_eq!(earlier(after, before), before);
    }

    /// ``UDSS_LLR_0019`` — the interval is the modular difference.
    #[test]
    fn interval_is_a_plain_difference_when_no_wrap_occurs() {
        assert_eq!(Timestamp(500).interval_since(Timestamp(200)), 300);
    }

    /// ``UDSS_LLR_0019`` — and stays correct across the 2^32 boundary, which is the
    /// whole reason the requirement names modular arithmetic rather than subtraction.
    #[test]
    fn interval_wraps_at_the_modulus() {
        assert_eq!(Timestamp(5).interval_since(Timestamp(u32::MAX)), 6);
    }

    /// ``UDSS_LLR_0019`` — a timestamp is zero milliseconds after itself.
    #[test]
    fn interval_from_itself_is_zero() {
        assert_eq!(Timestamp(7).interval_since(Timestamp(7)), 0);
    }

    /// A deadline is reached at its own instant and after it, and not before.
    #[test]
    fn a_deadline_is_reached_at_and_after_its_instant() {
        assert!(!Timestamp(99).has_reached(Timestamp(100)));
        assert!(Timestamp(100).has_reached(Timestamp(100)));
        assert!(Timestamp(101).has_reached(Timestamp(100)));
        assert_eq!(Timestamp(90).until(Timestamp(100)), 10);
        assert_eq!(Timestamp(100).until(Timestamp(100)), 0);
        assert_eq!(Timestamp(150).until(Timestamp(100)), 0);
    }

    /// ``UDSS_LLR_0019`` — across the wrap, a deadline just past it is still ahead of a
    /// time just before it, and one just before it has been reached by a time just past
    /// it. The derived `Ord` says the opposite of both.
    #[test]
    fn has_reached_and_until_hold_across_the_wrap() {
        let before = Timestamp(u32::MAX - 5);
        let after = Timestamp(10);
        assert!(!before.has_reached(after));
        assert!(
            before >= after,
            "the raw order, which is not the time order"
        );
        assert_eq!(before.until(after), 16);
        assert!(after.has_reached(before));
        assert!(after < before, "the raw order, which is not the time order");
        assert_eq!(after.until(before), 0);
    }

    /// The boundary of the half-range rule: 2^31 - 1 ms past is reached, 2^31 is ahead.
    #[test]
    fn half_the_range_is_the_boundary() {
        let deadline = Timestamp(0);
        assert!(Timestamp(u32::MAX / 2).has_reached(deadline));
        assert!(!Timestamp(u32::MAX / 2 + 1).has_reached(deadline));
        assert_eq!(
            Timestamp(u32::MAX / 2 + 1).until(deadline),
            u32::MAX / 2 + 1
        );
    }
}
