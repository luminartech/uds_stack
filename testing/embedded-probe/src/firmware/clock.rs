//! The `embassy-time` driver an integrator supplies: a 1 kHz `SysTick` counting
//! milliseconds, and a generic timer queue it wakes on every tick.

use core::cell::{Cell, RefCell};
use core::task::Waker;

use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::exception;
use critical_section::Mutex;
use embassy_time_driver::{Driver, TICK_HZ};
use embassy_time_queue_utils::Queue;

const CORE_HZ: u32 = 16_000_000;

struct SysTickDriver {
    ticks: Mutex<Cell<u64>>,
    queue: Mutex<RefCell<Queue>>,
}

embassy_time_driver::time_driver_impl!(static DRIVER: SysTickDriver = SysTickDriver {
    ticks: Mutex::new(Cell::new(0)),
    queue: Mutex::new(RefCell::new(Queue::new())),
});

impl Driver for SysTickDriver {
    fn now(&self) -> u64 {
        critical_section::with(|cs| self.ticks.borrow(cs).get())
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            self.queue.borrow_ref_mut(cs).schedule_wake(at, waker);
        });
    }
}

/// Starts the tick. `None` where the core peripherals were already taken.
pub(super) fn start() -> Option<()> {
    let mut syst = cortex_m::Peripherals::take()?.SYST;
    let reload = CORE_HZ.checked_div(u32::try_from(TICK_HZ).ok()?)?;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(reload.checked_sub(1)?);
    syst.clear_current();
    syst.enable_interrupt();
    syst.enable_counter();
    Some(())
}

#[exception]
fn SysTick() {
    critical_section::with(|cs| {
        let ticks = DRIVER.ticks.borrow(cs);
        ticks.set(ticks.get().saturating_add(1));
        DRIVER.queue.borrow_ref_mut(cs).next_expiration(ticks.get());
    });
}
