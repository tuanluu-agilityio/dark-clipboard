//! Best-effort zeroing of temporary buffers before they are freed.
//!
//! Volatile writes keep the compiler from removing the zeroing as a dead store.
//! Copies made earlier (for example by reallocation) are out of reach.

// The crate denies unsafe code; the volatile writes below are the one exception (F407-11).
#![allow(unsafe_code)]

use std::sync::atomic::{compiler_fence, Ordering};

/// Zeroes every byte of `s` (including spare capacity) and leaves it empty.
pub(crate) fn wipe(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    wipe_bytes(&mut bytes);
}

/// Zeroes every byte of `v`'s allocation (length and spare capacity) and leaves it empty.
/// The allocation is kept.
pub(crate) fn wipe_bytes(v: &mut Vec<u8>) {
    v.clear();
    for slot in v.spare_capacity_mut() {
        // SAFETY: `slot` is a valid, aligned, exclusively borrowed `MaybeUninit<u8>` inside
        // the vector's allocation; writing a `u8` to it is always sound.
        unsafe { std::ptr::write_volatile(slot.as_mut_ptr(), 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

/// Zeroes every byte of `buf` in place.
pub(crate) fn wipe_slice(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to an initialized `u8`.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_empties_string() {
        let mut s = String::from("temporary buffer");
        wipe(&mut s);
        assert!(s.is_empty());
        let mut e = String::new();
        wipe(&mut e);
        assert!(e.is_empty());
    }

    #[test]
    fn wipe_slice_zeroes() {
        let mut a = [7u8; 19];
        wipe_slice(&mut a);
        assert_eq!(a, [0u8; 19]);
    }

    #[test]
    fn wipe_bytes_zeroes_allocation() {
        let mut v = b"0123456789abcdef".to_vec();
        v.truncate(4);
        let cap = v.capacity();
        wipe_bytes(&mut v);
        assert!(v.is_empty());
        assert_eq!(v.capacity(), cap);
        // SAFETY: test-only; the allocation of `cap` bytes is still owned by `v`, and
        // `wipe_bytes` initialized all of them.
        let all = unsafe { std::slice::from_raw_parts(v.as_ptr(), cap) };
        assert!(all.iter().all(|&b| b == 0));
    }
}
