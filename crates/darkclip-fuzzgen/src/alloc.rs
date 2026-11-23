//! A global allocator that measures, per thread, the live heap growth of one fuzz call.
//!
//! Every harness test binary installs it:
//! `#[global_allocator] static A: darkclip_fuzzgen::TrackingAlloc = darkclip_fuzzgen::TrackingAlloc;`
//! It forwards every call to the system allocator unchanged; while a thread is armed it also
//! counts that thread's bytes allocated minus bytes freed, and keeps the peak. Other threads
//! and unarmed periods cost one thread-local read.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static CUR: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// Starts counting this thread's heap growth from zero.
pub fn arm() {
    let _ = CUR.try_with(|c| c.set(0));
    let _ = PEAK.try_with(|p| p.set(0));
    let _ = ARMED.try_with(|a| a.set(true));
}

/// Stops counting and returns the peak live growth since [`arm`], in bytes.
pub fn disarm() -> usize {
    let _ = ARMED.try_with(|a| a.set(false));
    let peak = PEAK.try_with(Cell::get).unwrap_or(0);
    usize::try_from(peak).unwrap_or(0)
}

fn track(delta: isize) {
    // `try_with`: never panics, also during thread teardown. These thread locals are
    // const-initialised without a destructor, so touching them never allocates.
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        let _ = CUR.try_with(|c| {
            let now = c.get().saturating_add(delta);
            c.set(now);
            let _ = PEAK.try_with(|p| {
                if now > p.get() {
                    p.set(now);
                }
            });
        });
    }
}

fn signed(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// The system allocator plus per-thread accounting (see the module docs).
pub struct TrackingAlloc;

// SAFETY: every method forwards to `System` with the caller's arguments unchanged, so the
// `GlobalAlloc` contract is the system allocator's; the accounting touches no allocation.
unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: same layout the caller passed to us, which the caller guarantees is valid.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            track(signed(layout.size()));
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            track(signed(layout.size()));
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        track(-signed(layout.size()));
        // SAFETY: `ptr` was returned by this allocator (that is, by `System`) with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr`/`layout` come from this allocator and `new_size` is the caller's
        // valid request, forwarded unchanged.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        // On failure the old block stays allocated and unchanged: nothing to count.
        if !p.is_null() {
            track(signed(new_size).saturating_sub(signed(layout.size())));
        }
        p
    }
}
