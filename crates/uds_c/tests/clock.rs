//! The `time-driver` clock over a simulated `uds_now_ms`.

#![allow(unsafe_code, reason = "implements the C clock uds_c calls")]

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time_driver::{Driver, TICK_HZ};
use uds_c::clock::Clock;

/// `uds_now_ms`: what the test sets.
static NOW_MS: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
extern "C" fn uds_now_ms() -> u32 {
    NOW_MS.load(Ordering::Relaxed)
}

/// The clock keeps counting up when the 32-bit millisecond counter wraps.
#[test]
fn the_clock_survives_the_millisecond_counter_wrapping() {
    let clock = Clock::new();
    NOW_MS.store(u32::MAX - 1, Ordering::Relaxed);
    let before = clock.now_ms();
    NOW_MS.store(2, Ordering::Relaxed);
    let after = clock.now_ms();
    assert_eq!(after.checked_sub(before), Some(4));
    assert_eq!(clock.now(), after.saturating_mul(TICK_HZ) / 1_000);
}
