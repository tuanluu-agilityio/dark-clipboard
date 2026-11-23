# Task 413: 0.1.0 release blockers (Task 407 findings)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/413-release-blockers` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-01, GC-03, GC-06, GC-10, GC-13, GC-14, GC-18, **GC-19**. **Source:** `docs/security/review-0.1.0.md` findings F407-01 to F407-05, F407-08, F407-11. Decision entry D-067.

## Requirements
1. **F407-01 (high, T-03): keep crash dumps out of Windows Error Reporting.**
   - At start-up, before the capture actor starts and on every run mode, the desktop app excludes itself from WER and sets the no-heap flag, using `WerAddExcludedApplication` and `WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP)`.
   - Check the exact signatures and semantics against Microsoft's documentation. The `windows` crate feature must be one already allowed by the gate (no networking features).
   - Darkshot's `crates/darkshot-windows/src/wer.rs` (`D:\2026 - Tuan\shotme`) may be read as a reference. Never modify Darkshot. Re-implement the code here; do not import it.
   - A failure is logged as a content-free warning and does not stop start-up.
   - Update threat-model T-03 with a dated "Changed by D-067" note saying what is really done.
   - Add an owner checklist row for the WER registry value.
2. **F407-02 (medium, T-04): data folder access.**
   - On Windows, the data folder (default or `--data-dir`) gets a protected DACL at start-up that grants only the current user and SYSTEM full control, inherited by its children. This lives in `apps/desktop` or `darkclip-windows`, not in storage (GC-01).
   - Apply it only to a folder Dark Clipboard created or owns. If setting it fails, log a content-free warning and continue.
   - Test the pure parts, such as the SDDL or ACE list builder. Add an owner row (`icacls` on the data folder).
3. **F407-03 (medium): a database that cannot be opened or is damaged.**
   - If the store cannot open (corrupt, not a database, or `SchemaTooNew`), the desktop shows a message window (i18n, en and vi) instead of quitting silently (data-model §7).
   - For corruption, the window offers "Move the damaged history aside and start empty". This renames `history.db` (and `-wal`/`-shm`) to `history.db.damaged-<ms>` and restarts. It never deletes anything.
   - `SchemaTooNew` only explains and quits, and never modifies the file.
   - Run `PRAGMA quick_check` at open. A failure is treated the same as corruption, and the read-only connection is not used.
   - Test the decision logic and the rename in temp folders.
4. **F407-04 (medium, T-12):**
   - Correct the threat-model T-12 text to match the code (current-user DACL, no peer-credential check), with a dated note.
   - If the single-instance pipe is held by a process the app cannot talk to, show a message ("Dark Clipboard could not start: another program is using its connection") instead of quitting silently.
5. **F407-05 (low):** the backup dialog, the README and the CHANGELOG say that a backup is a plain zip containing every clip, including "do not sync" ones, and that saving it in a synced folder (OneDrive, Dropbox) uploads it.
6. **F407-08 (low):** add KeePass 2 (`keepass.exe`) to the built-in excluded apps (privacy-design table, D-067), with the same removal confirmation text as the other password managers. Keep the R-02 notes honest (Bitwarden and 1Password are still untested).
7. **F407-11 (info):** add `#![forbid(unsafe_code)]` to every crate that has no `unsafe` today (`darkclip-platform`, `darkclip-storage`, `darkclip-app`, and core if its only unsafe is `wipe.rs`, in which case scope it to the other modules or use `deny`).

## Tests
All existing tests pass unchanged, plus tests for the pure parts above, written first and seen failing. Do not run `#[ignore]` tests. Never launch the UI (GC-19); never write the real data folder's ACL in tests (temp folders only).

## Not doing
macOS and Linux; the search bench (F-21); third-party notices (Task 408/410); a full crash reporter.
