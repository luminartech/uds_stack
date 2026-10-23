//! `embassy-time`'s clock over `uds_now_ms`, for an image whose server is polled on
//! every pass of the firmware's loop.

use core::sync::atomic::{AtomicU32, Ordering};
use core::task::Waker;

use embassy_time_driver::{Driver, TICK_HZ};

use crate::sys;

/// Time since `uds_now_ms`'s zero, widened to 64 bits across the 32-bit counter's wraps
/// and given in `embassy-time` ticks.
///
/// A wrap is seen when a reading is lower than the one before it, so the clock is read
/// at least once per wrap of the counter: every 49 days at one count per millisecond.
/// It schedules no wakeups, since the server is polled on every pass and reads the time
/// on each poll. Read only from the server's execution context, which
/// [`Layers::new`](crate::Layers::new) makes one: two contexts could each see one wrap
/// and count it twice. Installed by [`time_driver!`](crate::time_driver).
#[derive(Debug)]
pub struct Clock {
    last: AtomicU32,
    wraps: AtomicU32,
}

impl Clock {
    /// The clock, before its first reading.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: AtomicU32::new(0),
            wraps: AtomicU32::new(0),
        }
    }

    /// Milliseconds since `uds_now_ms`'s zero, counting its wraps. Loads and stores
    /// rather than read-modify-writes: one execution context, and a core without
    /// compare-and-swap.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        let ms = sys::uds_now_ms();
        let mut wraps = self.wraps.load(Ordering::Relaxed);
        if ms < self.last.load(Ordering::Relaxed) {
            wraps = wraps.wrapping_add(1);
            self.wraps.store(wraps, Ordering::Relaxed);
        }
        self.last.store(ms, Ordering::Relaxed);
        (u64::from(wraps) << 32) | u64::from(ms)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Driver for Clock {
    fn now(&self) -> u64 {
        self.now_ms().saturating_mul(TICK_HZ) / 1_000
    }

    fn schedule_wake(&self, _at: u64, _waker: &Waker) {}
}

/// Installs [`Clock`] as the image's `embassy-time` driver. Invoked once, in the binary:
/// an image links one driver.
#[macro_export]
macro_rules! time_driver {
    () => {
        $crate::__private::embassy_time_driver::time_driver_impl!(
            static UDS_C_CLOCK: $crate::clock::Clock = $crate::clock::Clock::new()
        );
    };
}
