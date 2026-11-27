# Task 600: Phase 6 decisions and network gate (translate in place)

**Agent:** `scaffolder` · **Model:** sonnet · **Branch:** `task/600-phase6-decisions` (from `main`; it already carries the spike, design, prototype and plan commits) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-08 (amended here), GC-17, GC-18, GC-19. **Source:** owner approval 2026-11-27 ("Duyệt tất cả"; answers in `docs/specs/translate-in-place/design.md` §9). Plan: `docs/plans/phase-6-translate-in-place.md`.

## Goal
Record the owner's Phase 6 decisions in the project's decision log and the approved documents' dated notes, and change the quality gate so that exactly one more file, `apps/desktop/src/ai_net.rs`, may use network APIs. Also fix F-41, which makes the Windows manifest check silently empty. This task adds no product code.

## Facts
- The next free decision numbers are **D-093** and **D-094** (the last is D-092, `memory-bank/decisions.md`). Use the table row format of the existing entries.
- Owner answers (2026-11-27): Q1 consent replaced by an enable-time dialog plus a per-press overlay "Sending N characters to Anthropic…", with no per-press dialog. Q2: a second network file is allowed. Q3: Phase 6 runs before Paste Stack (0.2). The owner approved the design, the prototype and the plan.
- Approved documents are not rewritten (`CLAUDE.md`). Append a dated "Changed by D-NNN, 2026-11-27: …" note where the text changes meaning.
- `scripts/check.ps1` ~l.118–137 and `scripts/check.sh` ~l.58–66 hold the two GC-08 checks to change. The `.rs` check allowlists only `apps/desktop/src/update.rs`.
- F-41 (`memory-bank/activeContext.md`): `check.ps1:121` uses `-notmatch '\target\'`. That is an invalid regex, so `$tomls` stays empty and the manifest check checks nothing on Windows. `check.sh` is correct.
- `ai_net.rs` does not exist yet (Task 602 creates it). The allowlist must name the exact path. It must not use a pattern such as `apps/desktop/src/*.rs`.

## Steps
1. **D-093** (consent for translate in place): the enable-time consent dialog plus the per-press overlay replace the per-request confirmation of `docs/architecture/ai-integration.md` §4 **for the translate-in-place flow only**. Other future AI flows keep §4. Text never sent: excluded apps, mandatory markers, `NoCloud`, transient or auto-generated content, sensitive hits, Private mode, text over 8000 chars. Off by default.
2. **D-094** (GC-08 amendment): network APIs are allowed in `apps/desktop/src/update.rs` and `apps/desktop/src/ai_net.rs` only. `ai_net.rs` may reach only `https://api.anthropic.com/v1/messages` (no redirects, HTTPS only, 30 s timeout, 256 KiB cap), and only from the translate flow. `crates/` stay network-free, including the new `darkclip-ai`. Also record that Phase 6 runs before Paste Stack (owner, Q3).
3. Dated notes (one or two lines each, "Changed by D-093/D-094, 2026-11-27"):
   - `docs/plans/global-constraints.md` GC-08;
   - `docs/architecture/ai-integration.md` §4;
   - `docs/security/threat-model.md`: T-13 (consent per D-093); a new **T-21** "Translate request to Anthropic (B6)" with its mitigations taken from the design §6 and the plan's Global Constraints; T-20 (a second network file);
   - `docs/security/privacy-design.md` §9 and the promise-1 line (opt-in exception, like the D-057 note);
   - `docs/plans/roadmap.md` (Phase 6 row: translate in place first, plan file, before Paste Stack).
4. Gate, in both `check.ps1` and `check.sh`:
   - allowlist exactly `apps/desktop/src/ai_net.rs` next to `update.rs` in the `.rs` network-API check;
   - rename the check's heading and failure text to "network APIs only in update.rs and ai_net.rs (GC-08, D-057, D-094)".
5. F-41: fix the `check.ps1` manifest filter (for example `-notmatch '\\target\\'`, or `[regex]::Escape('\target\')`). Prove it works: a temporary `crates/darkclip-core/Cargo.toml` line `x = { package = "ureq" }` fails, and the real manifests pass. Restore the file afterwards. Record the command and output in the report.
6. Self-test the allowlist on both scripts:
   - a temporary `apps/desktop/src/ai_net.rs` containing `use ureq::Agent;` passes the `.rs` check;
   - the same line in a temporary `apps/desktop/src/other_net.rs` fails it.
   Remove both files afterwards. (Do not add them to `mod` trees; the scan reads files.) Run `bash scripts/check.sh` only if bash is available, otherwise say so. Record the commands and output.
7. Memory-bank: close F-41 in `activeContext.md`. Add the Phase 6 rows to `progress.md` (done in this commit by the orchestrator; just update the 600 row). Note the `ai_net.rs` allowlist in `systemPatterns.md` under the gate patterns.

## Files this task may edit
`memory-bank/decisions.md`, `memory-bank/activeContext.md`, `memory-bank/progress.md`, `memory-bank/systemPatterns.md`, `docs/plans/global-constraints.md`, `docs/plans/roadmap.md`, `docs/architecture/ai-integration.md`, `docs/security/threat-model.md`, `docs/security/privacy-design.md`, `scripts/check.ps1`, `scripts/check.sh`. Temporary files from steps 5–6 must be gone before the commit.

## Acceptance
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- The step 5 and 6 self-tests are shown with their exact commands and outputs.
- After the F-41 fix, the manifest check visibly lists manifests (or counts them) on Windows.
- No product code changed (`git diff main --stat` shows only the files above plus the earlier spike/design/plan commits).

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Release scripts:** PowerShell scripts end with `exit 0` (a leaked `$LASTEXITCODE`, PR #32). Keep the gate's exit codes intact.
- **Gate drift:** `check.ps1` and `check.sh` must stay equivalent. CI runs `check.sh` on macOS and Ubuntu and `check.ps1` on Windows.
- **GC-19:** never run the app against the default data dir, and never write the real clipboard.
