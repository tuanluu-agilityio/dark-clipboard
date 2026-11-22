//! Best-effort wiping of content buffers.

use darkclip_core::RawRepresentation;

/// Best-effort overwrite of content buffers on paths that discard them.
pub(super) fn wipe_bytes(v: &mut [u8]) {
    v.fill(0);
    std::hint::black_box(&*v);
}

pub(super) fn wipe_string(s: &mut String) {
    let mut v = std::mem::take(s).into_bytes();
    wipe_bytes(&mut v);
}

pub(super) fn wipe_reps(reps: &mut [RawRepresentation]) {
    for r in reps {
        wipe_bytes(&mut r.bytes);
    }
}
