# Task 607: Translate in place: verification rows and documentation

**Agent:** `platform-verifier` · **Model:** sonnet · **Branch:** `task/607-translate-docs` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14, GC-18, GC-19; `CLAUDE.md` "Honest platform claims"; D-093, D-094, D-095. Use the `platform-capability-check` skill before writing any support claim. Plan: `docs/plans/phase-6-translate-in-place.md`. Spec: `docs/specs/translate-in-place/design.md`. What was built: Tasks 600–606 (`memory-bank/progress.md` Phase 6 rows; packages in `docs/tasks/60*.md`).

## Goal
Make every document that describes what Dark Clipboard does, and what leaves the device, true for `main` after Phase 6. Add the owner's manual test rows. Claim nothing as tested that no one has tested.

## Steps
1. **`docs/testing/platform-checklist.md`:** add a "Translate in place (Phase 6)" section of owner rows, all `untested`. Take the rows from the Task 605 and 606 reports, which are summarised in the progress rows and packages:
   - overlay focus in Notepad, Zalo, Slack and Chrome (the caret stays, the paste lands, ✕ does not take focus, no `translate_overlay_focus` log line);
   - selection versus whole field; Ctrl+Z; history stays clean (neither the source nor the translation is stored); previous clipboard restored;
   - Paused and capture-off still translate and store nothing; Private is refused;
   - an excluded app (KeePassXC) is refused; a terminal is refused and its running program is not interrupted;
   - Esc during sending; a second press is ignored; held keys are refused;
   - a newer copy during the wait wins (`clipboard_changed`);
   - offline, wrong key, no key;
   - consent dialog Cancel, Esc and Turn on; key Save, Replace and Remove (mask only); Test button;
   - shortcut clash with the popup;
   - DPI 100 and 150 %, two monitors; Vietnamese text; Windows animations off.
   Note that G1 (the spike) passed in Zalo, Slack and VS Code. That was the **spike**, not the product, so no product row is marked from it.
2. **`docs/architecture/platform-compatibility.md`:** add a "Translate in place" row. Windows is `untested` (built and unit-tested, owner rows pending); macOS and Linux are `unavailable` (not built). Add a dated note naming D-093, D-094 and D-095.
3. **`README.md`:**
   - Fix the privacy paragraph (line ~132, "The app makes no network request, with one opt-in exception: the update check"). There are now two opt-in exceptions. For translate in place, say:
     - what is sent: the selected text or the whole field, up to 8000 characters, plus the fixed prompt, to `api.anthropic.com`, with your key;
     - when: only on the shortcut press, or on the Settings Test button with the fixed sample;
     - that it is off by default, and that the consent dialog appears when you turn it on;
     - what is never sent (the Settings "Never sent" card);
     - where the key lives (Windows Credential Manager);
     - the proxy behaviour (environment proxies including NO_PROXY; the system proxy is not used);
     - that Anthropic's data policy applies.
   - Keep the "No account, no cloud, no telemetry" headline only if it stays true. It says no cloud *by default*; reword minimally, for example "No account, no telemetry; no cloud unless you turn on translate in place".
   - Add a short "Translate in place (Windows, alpha, unreleased)" usage section: enable it in Settings → Translate, then Ctrl+Alt+T; selection or whole field; Ctrl+Z; Esc.
4. **`CHANGELOG.md` `[Unreleased]`:**
   - **Added:** translate in place (Windows), with its settings, the consent dialog, the key in Credential Manager and the Test button; the new skip reason "Translate in place source".
   - **Fixed:** F-41 (Windows gate manifest check).
   - **Known issues**, each in one line:
     - VS Code copies the current line when nothing is selected, so the line is duplicated;
     - apps running as administrator are refused;
     - only text survives the restore (images and HTML on the clipboard are not restored, and the restore puts back the stored clip only);
     - Esc during the copy wait can leave the source text on the clipboard (not stored, not sent);
     - Esc is held globally while a translation runs, up to about 30 s;
     - a copy you make in the same app within 1.5 s of pressing translate, before the translation is written, is not stored in history;
     - if the clipboard owner is a child process (some Electron/WebView2 apps), the claim cannot match, and in Normal capture mode the source text is stored as a normal clip (I1 R2);
     - plaintext copies of the text inside the AI client are not wiped from memory (T-21 R6);
     - Notepad, Chrome and Word are not yet tested.
5. **`website/`: no change in this task.** The site describes the released 0.2.0, where the update check is still the only network exception. Record a follow-up in `memory-bank/activeContext.md`: "At the release that ships translate in place, update `website/index.html` f2b/f2s ('No network … One opt-in update check'), `desc` and `lede` ('no cloud') in EN and VI."
6. **Owner gate:** add an open owner row to the `memory-bank/activeContext.md` "Waiting on owner" table: "Q-08: Translate in place manual rows (platform-checklist §Translate) before any release that ships it."
7. `memory-bank/progress.md` (the 607 row) and `memory-bank/activeContext.md` (next: 608 phase review).

## Files this task may edit
`docs/testing/platform-checklist.md`, `docs/architecture/platform-compatibility.md`, `README.md`, `CHANGELOG.md`, `memory-bank/activeContext.md`, `memory-bank/progress.md`. Nothing under `crates/`, `apps/` or `website/`.

## Acceptance
- Every claim in README, CHANGELOG and the matrix matches the code on `main` and the decisions (D-093, D-094, D-095). Each claim the reviewer checks has a cited file.
- No row is marked tested, and the spike is not cited as product testing.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED` (docs only; the gate must still pass).

## Known pitfalls (from `memory-bank/systemPatterns.md` and earlier reviews)
- **Honest platform claims:** only `T` in the matrix counts as supported. F-40 (README vs matrix wording on KeePassXC) is a precedent: README and the matrix must say the same thing.
- **Flags that follow content:** the export/backup wording about `CanUploadToCloudClipboard` must stay consistent with "translate never sends NoCloud content".
- **GC-19:** do not run the app with a visible window, take screenshots or send input; the rows are for the owner.
