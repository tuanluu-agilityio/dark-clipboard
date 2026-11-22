# Task 107: Windows verification against real apps

**Agent:** `platform-verifier` · **Model:** sonnet · **Branch:** `task/107-windows-verification` · **Review:** `reviewer` (opus)
**Needs the owner at the machine.** The verifier prepares, the owner performs copies, the verifier records. The verifier never simulates keyboard or mouse input.
**Global Constraints:** GC-06, GC-07. **Plan:** Task 107 row. Skill: `platform-capability-check`.

## Files
Create `docs/testing/platform-checklist.md` (Windows column now; macOS/Linux columns `untested`). Update `docs/architecture/platform-compatibility.md` Windows rows to **T** only where a row passed, with a dated "Changed by D-NNN" note. Possibly `crates/darkclip-core/src/policy.rs` built-in excluded apps (only via a decision entry).

## Steps
1. Run `cargo test -p darkclip-windows -- --ignored` **with the owner's consent**, after telling them it overwrites the clipboard. Record the output.
2. Start `dark-clipboard --headless --data-dir <scratch>`. For each row the owner copies; the verifier inspects the scratch DB (counts, kinds, source, skip counters; never prints content) and records T/F, date, app version.
3. Rows: Darkshot Copy image, Darkshot Copy link, KeePassXC (password, username), Bitwarden desktop, 1Password (if installed), Word/Excel text and table, Edge/Chrome HTML with Vietnamese text, Explorer copy of files, Snipping Tool image, RDP (if available), elevated app (source `None`), Win+V history unaffected by Dark Clipboard copy-back (D-012).
4. A password manager without markers → add it to built-in excluded apps (D-entry), and record risk R-02 status.

## Acceptance
Every row recorded T (with date and version) or "not available". No row claimed without evidence.
