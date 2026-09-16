//! The timebase: what a timestamp is, and how an interval is computed from two.
//!
//! ``UDSS_LLR_0017`` forbids the session layer to read a clock, so every value here
//! originates with the caller.

/// A point in time, as the caller reports it.
///
/// ``UDSS_LLR_0018`` — a 32-bit unsigned count of milliseconds. ``UDSS_LLR_0020``
/// accompanies every input with one.
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
}

#[cfg(test)]
mod tests {
    use super::Timestamp;

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
}
