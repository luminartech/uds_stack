//! The response records the server owns and lends to the C handler it is calling. A
//! handler fills one only through [`uds_record_copy`], which bounds every copy by the
//! record's length, so C never writes server memory itself.

#![allow(
    unsafe_code,
    reason = "exports uds_record_copy and copies through its pointers"
)]

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// The record lent to the handler being called: its start, its length and how far a copy
/// has reached. A null start while no handler holds one.
static LENT: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static LENT_LEN: AtomicUsize = AtomicUsize::new(0);
static LENT_FILLED: AtomicUsize = AtomicUsize::new(0);

/// A record of `buf`'s length, kept across a handler's calls: what a pending handler
/// copied stays until the request completes.
#[derive(Debug)]
pub(crate) struct Record<'a> {
    buf: &'a mut [u8],
    filled: usize,
}

impl<'a> Record<'a> {
    /// A record over `buf`, zeroed, its whole length the bound.
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        buf.fill(0);
        Self { buf, filled: 0 }
    }

    /// The whole record, zero where nothing was copied.
    pub(crate) fn all(&self) -> &[u8] {
        self.buf
    }

    /// The record up to the furthest byte copied.
    pub(crate) fn filled(&self) -> &[u8] {
        self.buf.get(..self.filled).unwrap_or_default()
    }

    /// Calls `call` with this record lent to it, for `uds_record_copy` to fill.
    pub(crate) fn lend<R>(&mut self, call: impl FnOnce() -> R) -> R {
        /// Takes the record back however `call` returns.
        struct Lent;

        impl Drop for Lent {
            fn drop(&mut self) {
                LENT.store(ptr::null_mut(), Ordering::Relaxed);
            }
        }

        LENT_LEN.store(self.buf.len(), Ordering::Relaxed);
        LENT_FILLED.store(self.filled, Ordering::Relaxed);
        LENT.store(self.buf.as_mut_ptr(), Ordering::Relaxed);
        let lent = Lent;
        let result = call();
        drop(lent);
        self.filled = LENT_FILLED.load(Ordering::Relaxed);
        result
    }
}

/// `uds_record_copy`: copies `len` bytes from `src` into the record of the request being
/// served, at byte `offset`. Returns false, copying nothing, outside a handler that
/// answers with a record, for a NULL `src`, or for a copy past the record's bound.
///
/// # Safety
///
/// `src` is NULL or points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uds_record_copy(
    offset: usize,
    src: *const u8,
    len: usize,
) -> bool {
    let buf = LENT.load(Ordering::Relaxed);
    if buf.is_null() || src.is_null() {
        return false;
    }
    let Some(end) = offset
        .checked_add(len)
        .filter(|&end| end <= LENT_LEN.load(Ordering::Relaxed))
    else {
        return false;
    };
    // SAFETY: `buf` is the start of the record `Record::lend` lent for this call, valid
    // and unaliased for `LENT_LEN` bytes, of which `offset..end` lies within; `src` is
    // `len` readable bytes by the caller's contract. `ptr::copy` admits a source inside
    // the record.
    unsafe { ptr::copy(src, buf.add(offset), len) };
    LENT_FILLED.store(
        LENT_FILLED.load(Ordering::Relaxed).max(end),
        Ordering::Relaxed,
    );
    true
}

/// `uds_record_copy_16_be`: copies `value` big-endian, two bytes, into the record at
/// byte `offset`, as [`uds_record_copy`] does.
#[unsafe(no_mangle)]
pub extern "C" fn uds_record_copy_16_be(offset: usize, value: u16) -> bool {
    copy_be(offset, &value.to_be_bytes())
}

/// `uds_record_copy_32_be`: copies `value` big-endian, four bytes, into the record at
/// byte `offset`, as [`uds_record_copy`] does.
#[unsafe(no_mangle)]
pub extern "C" fn uds_record_copy_32_be(offset: usize, value: u32) -> bool {
    copy_be(offset, &value.to_be_bytes())
}

/// `uds_record_copy_64_be`: copies `value` big-endian, eight bytes, into the record at
/// byte `offset`, as [`uds_record_copy`] does.
#[unsafe(no_mangle)]
pub extern "C" fn uds_record_copy_64_be(offset: usize, value: u64) -> bool {
    copy_be(offset, &value.to_be_bytes())
}

/// Copies an integer's big-endian bytes.
fn copy_be(offset: usize, bytes: &[u8]) -> bool {
    // SAFETY: `bytes` is readable for its length.
    unsafe { uds_record_copy(offset, bytes.as_ptr(), bytes.len()) }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::*;

    /// The lent record is one per image, so these tests take turns.
    fn serial() -> MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A copy as a handler makes it.
    fn copy(offset: usize, bytes: &[u8]) -> bool {
        // SAFETY: `bytes` is readable for its length.
        unsafe { uds_record_copy(offset, bytes.as_ptr(), bytes.len()) }
    }

    #[test]
    fn a_copy_within_the_bound_lands_at_its_offset() {
        let _serial = serial();
        let mut buf = [0xAA_u8; 4];
        let mut record = Record::new(&mut buf);
        assert!(record.lend(|| copy(1, &[7, 8])));
        assert_eq!(record.all(), [0, 7, 8, 0]);
        assert_eq!(record.filled(), [0, 7, 8]);
    }

    #[test]
    fn a_copy_past_the_bound_is_refused_and_copies_nothing() {
        let _serial = serial();
        let mut buf = [0_u8; 4];
        let mut record = Record::new(&mut buf);
        assert!(!record.lend(|| copy(3, &[1, 2])));
        assert!(!record.lend(|| copy(usize::MAX, &[1])));
        assert_eq!(record.all(), [0; 4]);
        assert_eq!(record.filled(), [0_u8; 0]);
    }

    #[test]
    fn a_copy_outside_a_handler_or_from_null_is_refused() {
        let _serial = serial();
        assert!(!copy(0, &[1]));
        let mut buf = [0_u8; 4];
        let mut record = Record::new(&mut buf);
        // SAFETY: NULL is part of the contract.
        assert!(!record.lend(|| unsafe { uds_record_copy(0, ptr::null(), 1) }));
        record.lend(|| ());
        assert!(!copy(0, &[1]), "taken back after the call");
    }

    #[test]
    fn integers_land_big_endian_within_the_bound() {
        let _serial = serial();
        let mut buf = [0_u8; 14];
        let mut record = Record::new(&mut buf);
        assert!(record.lend(|| uds_record_copy_16_be(0, 0x0102)));
        assert!(record.lend(|| uds_record_copy_32_be(2, 0x0304_0506)));
        assert!(record.lend(|| uds_record_copy_64_be(6, 0x0708_090A_0B0C_0D0E)));
        assert_eq!(
            record.all(),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
        );
        assert!(
            !record.lend(|| uds_record_copy_32_be(11, 0)),
            "past the bound"
        );
        assert!(!uds_record_copy_16_be(0, 0), "outside a handler");
    }

    #[test]
    fn copies_across_calls_accumulate() {
        let _serial = serial();
        let mut buf = [0_u8; 4];
        let mut record = Record::new(&mut buf);
        assert!(record.lend(|| copy(2, &[3])));
        assert!(record.lend(|| copy(0, &[1])));
        assert_eq!(record.filled(), [1, 0, 3]);
    }
}
