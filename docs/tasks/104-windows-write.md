# Task 104: Windows `write(reps)` (copy-back)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/104-windows-write` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-10, GC-13. **Plan:** Task 104 row, note on D-012, amendment S-2; Review Focus 5.

## Read first
Spike answers 8, 9; `crates/darkclip-app/src/capture.rs` (copy-back, `last_self_write_seq`); `crates/darkclip-windows/src/{access,win,dib}.rs`.

## Files
Modify `crates/darkclip-windows/src/{access.rs, win.rs}`; add the PNG → DIB/DIBV5 encoder to `dib.rs`.

## Requirements
1. **S-2:** `write` is called on the actor thread. `WinAccess` lazily creates its own message-only owner window on the first `write` (on the calling thread; record the thread id and return `Backend` if later called from another thread) and destroys it on drop. `OpenClipboard(owner)`: one attempt, `Busy` otherwise.
2. `EmptyClipboard`, then `SetClipboardData` for every rep: plain text → `CF_UNICODETEXT` (UTF-16 + NUL); HTML → `HTML Format` (bytes as stored); RTF → `Rich Text Format`; `image/png` → registered `PNG` + `CF_DIBV5` (32-bit, alpha) + `CF_DIB` (32-bit BI_RGB); uri-list → `CF_HDROP` (`DROPFILES`, wide) only for `file://` URIs, else skip that rep. Memory from `GlobalAlloc(GMEM_MOVEABLE)`; on a `SetClipboardData` failure free the handle we still own. Any failure after `EmptyClipboard` → close and return `Backend`.
3. Read `GetClipboardSequenceNumber()` **before** `CloseClipboard` and return it (self-write suppression; spike 8). Document why in a comment.
4. **D-012:** never add any exclusion or history marker formats.
5. Over-size writes are not capped here (the clip was already capped at capture).

## Tests
- Pure: `png_to_dibv5_roundtrip` (encode, then decode with Task 103's DIB reader, equal pixels), `uri_list_to_dropfiles_layout` (offsets, double-NUL), `non_file_uris_skipped`.
- Fake OS seam: `writes_png_dibv5_dib_for_image`, `no_marker_formats_written`, `returns_seq_read_before_close`, `failure_after_empty_frees_handles`, `busy_once`, `wrong_thread_rejected`.
- `#[ignore = "writes the real clipboard"]`: `real_clipboard_write_copyback_suppressed` (write → monitor signal seq equals returned seq), `real_clipboard_write_image_readable_by_read` (write PNG, read back via Task 103). Do not run ignored tests.

## Not doing
Paste injection (Phase 3).
