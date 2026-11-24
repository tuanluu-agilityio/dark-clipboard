# Progress and token ledger

Updated: 2026-11-24 (0.1.0 released)

Statuses: `pending`, `in progress`, `review`, `done`. Tokens are the numbers subagents report when they finish. Orchestrator tokens cannot be measured per task.

## Phase 0

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| 0 Discovery, spikes, blueprint | main (b2b9603) | main session | opus | done (awaiting owner approval) | not measured | | | |

## Phase 1: Foundation

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| 001 Workspace bootstrap | task/001-workspace-bootstrap | scaffolder | sonnet | done | 59,266 | 29,555 (reviewer sonnet; fix verified by orchestrator) | 65,321 | 154,142 |
| 002 Core model, i18n | task/002-core-model-i18n | builder | sonnet | done | 97,136 | 47,499 (reviewer sonnet: APPROVED) | 102,081 | 246,716 |
| 003 Core policy | task/003-core-policy | specialist | opus | done | 95,465 | 58,907 (security-reviewer opus: APPROVED) | 0 | 154,372 |
| 004 Classify, hash | task/004-core-classify-hash | builder | sonnet | done | 79,309 | 27,404 (reviewer sonnet: APPROVED) | 83,194 | 189,907 |
| 005 Sensitive detection | task/005-core-sensitive | specialist | opus | done | 124,994 | 167,147 (security-reviewer opus ×2: should-fix → APPROVED-equivalent; round-2 fix verified by orchestrator) | 339,666 (2 rounds) | 631,807 |
| 006 Platform traits, fake | task/006-platform-traits-fake | builder | sonnet | done | 91,759 | 172,568 (specialist opus: CHANGES REQUIRED → APPROVED) | 100,426 | 364,753 |
| 007 Storage schema | task/007-storage-schema | specialist | opus | done | 154,499 | 169,359 (reviewer opus: CHANGES REQUIRED → APPROVED) | 178,797 | 502,655 |
| 008 Storage search | task/008-storage-search | builder | sonnet | done | 83,809 | 38,523 (reviewer sonnet: APPROVED) | 90,645 | 212,977 |
| 009 Storage retention | task/009-storage-retention | builder | sonnet | done | 94,709 | 53,101 (reviewer opus: APPROVED) | 102,230 | 250,040 |
| 010 Capture actor | task/010-app-capture-actor | specialist | opus | done | 180,356 | 374,129 (security-reviewer opus ×3: CR → CR → APPROVED) | 506,097 (2 rounds) | 1,060,582 |
| 011 App API, settings | task/011-app-api-settings | builder | sonnet | done | 152,545 | 241,464 (security-reviewer opus ×2: CR → APPROVED; reviewer upgraded from sonnet) | 406,538 (2 rounds) | 800,547 |
| 012 Phase review | main | specialist | opus | done: CHANGES REQUIRED (B-1 bench; later shown to be a measurement artifact) → Task 013 | | 195,016 | | 195,016 |
| 013 Phase 1 review fixes | task/013-phase1-fixes | specialist | opus | done | 139,586 | 38,199 (reviewer opus: APPROVED) | 181,043 | 358,828 |

## Phase 2: Windows capture

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| 101 Windows monitor | task/101-windows-monitor | specialist | opus | done | 144,728 | 58,984 (reviewer opus: APPROVED, 3 nits) | 0 | 203,712 |
| 105 Windows source | task/105-windows-source | builder | sonnet | done | 76,585 | 42,947 (security-reviewer opus: APPROVED, 2 should-fix fixed) | 7,255 | 126,787 |
| 102 Windows snapshot_meta | task/102-windows-snapshot-meta | specialist | opus | done | 146,190 | 82,490 (security-reviewer opus: APPROVED, 1 should-fix fixed) | 9,263 | 237,943 |
| 103 Windows read | task/103-windows-read | specialist | opus | done | 169,040 | 103,414 (reviewer opus ×2: APPROVED with 5 should-fix → fixed, APPROVED) | 48,271 | 320,725 |
| 103b App image decode | task/103b-app-image-decode | builder | sonnet | done | 95,352 | 70,837 (reviewer opus ×2: CHANGES REQUIRED (should-fix) → APPROVED) | 18,014 | 184,203 |
| 104 Windows write | task/104-windows-write | specialist | opus | done | 178,113 | 98,630 (reviewer opus ×2: CHANGES REQUIRED (2 blocking: post-close seq, per-write owner window) → APPROVED) | 28,935 | 305,678 |
| 106 Headless binary | task/106-headless-binary | builder | sonnet | done | 141,048 | 92,504 (reviewer opus ×2: APPROVED with 5 should-fix → fixed, APPROVED) | 21,895 | 255,447 |
| 107 Windows verification | task/107-windows-verification | orchestrator (interactive, D-041) | opus | done | not measured (orchestrator) | 79,051 (reviewer opus ×3: CR (over-claims) → CR (1 cell) → fixed) | 0 | 79,051 |
| 109 OLE source carry (F-14) | task/109-ole-source-carry | specialist | opus | done | 149,387 | 67,756 (security-reviewer opus ×2: CR (1 blocking: dequeue-time chain gate) → APPROVED) | 29,589 | 246,732 |
| 108 Phase review | main | specialist | opus | done: CHANGES REQUIRED (1 blocking: unbounded copy-back PNG decode; 4 should-fix) → Task 110 | | 225,146 | | 225,146 |
| 110 Phase 2 review fixes | task/110-phase2-fixes | specialist | opus | done | 265,048 | 91,107 (security-reviewer opus ×2: APPROVED with 1 should-fix → fixed, APPROVED) | 9,452 | 365,607 |

Phase 2 complete 2026-11-23 (Bitwarden/1Password rows n/a on the test machine; see activeContext).

## Phase 3: Desktop shell (Slint)

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| 200 Split capture.rs | task/200-split-capture | specialist | opus | done | 132,555 | 50,869 (security-reviewer opus: APPROVED, mechanical equivalence check) | 0 | 183,424 |
| 201 Desktop skeleton | task/201-desktop-skeleton | builder | sonnet | done | 147,056 | 116,203 (reviewer opus ×2: CR (2 blocking: shutdown hang, CI) → APPROVED) | 32,543 | 295,802 |
| 202 Tray | task/202-tray | builder | sonnet | done | 125,457 | 83,766 (reviewer opus ×2: CR (blocking: i64::MAX pause corrupts settings) → APPROVED) | 16,124 | 225,347 |
| 203 Shortcut | task/203-shortcut | builder | sonnet | done | 124,733 | 62,231 (reviewer opus: APPROVED, nits fixed) | 4,061 | 191,025 |
| 204 Popup | task/204-popup | specialist | opus | done | 335,455 | 151,701 (reviewer opus ×2: APPROVED with should-fix → fixed, APPROVED) | 37,299 | 524,455 |
| 205 Paste back | task/205-paste-back | specialist | opus | done | 225,505 | 95,918 (security-reviewer opus ×2: APPROVED with should-fix → fixed, APPROVED) | 13,786 | 335,209 |
| 206 Previews | task/206-previews | builder | sonnet | done | 230,796 | 124,502 (security-reviewer opus ×2: APPROVED with should-fix → fixed, APPROVED) | 40,923 | 396,221 |
| 211 History window | task/211-history-window | builder | sonnet | done | 267,633 | 120,811 (reviewer opus: APPROVED, fixes applied) | 9,697 | 398,141 |
| 207 Settings window | task/207-settings | builder | sonnet | done | 228,434 | 121,072 (security-reviewer opus ×2: APPROVED with should-fix → fixed, APPROVED) | 36,293 | 385,799 |
| 208 Autostart + welcome | task/208-autostart-welcome | builder | sonnet | done | 184,991 | 138,156 (security-reviewer opus ×2: CR (blocking: read-only defaults enabled capture) → APPROVED) | 48,074 | 371,221 |
| 209 ADR-0001 revisit | task/209-adr0001-revisit | platform-verifier | sonnet | done (NFR-05/NFR-04 not met with the GPU renderer → Task 212) | 64,210 | 72,411 (reviewer opus: CR (invalid runs: welcome visible, BOM settings) → fixed) | 34,759 | 171,380 |
| 212 Software renderer, BOM, timing | task/212-renderer-memory | specialist | opus | done (NFR-04, NFR-05 met on Windows) | 179,641 | 65,427 (reviewer opus: APPROVED, wording fixed) | 5,255 | 250,323 |
| 210 Alpha docs, artifact, phase review | task/210-alpha-docs | scaffolder (sonnet) + specialist (opus) | sonnet + opus | done: docs + artifact; phase review CHANGES REQUIRED (1 blocking: popup shows stale rows after hide) → Task 213 | 93,903 | 220,611 (phase review, specialist opus) | | 314,514 |
| 213 Phase 3 review fixes | task/213-phase3-fixes | specialist | opus | done | 220,569 | 102,230 (security-reviewer opus: APPROVED; login re-enable paths fixed) | 16,126 | 338,925 |

Phase 3 complete 2026-11-23 (all desktop UI owner rows still untested by hand; see activeContext).

## Phase 5: P1 organization, hardening (Windows scope first, D-057)

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| 401 Migration 2 | task/401-migration-2 | specialist | opus | done | 252,014 | 127,875 (security-reviewer opus: APPROVED with 5 should-fix → fixed, verified by orchestrator gate) | 29,644 | 409,533 |
| 402 Organization API | task/402-organization-api | builder | sonnet | done | 126,035 | 95,857 (reviewer opus: APPROVED with 4 should-fix → fixed; first commit had tests and code together, TDD not evidenced, fix round red-first) | 10,989 | 232,881 |
| 404 Transforms + edit copy | task/404-transforms-edit | builder | sonnet | done | 255,798 | 133,898 (security-reviewer opus ×2: CR (blocking: saving an edit could bump/modify the original) → APPROVED) | 26,759 | 416,455 |
| 411 Data management | task/411-data-management | builder | sonnet | done | 216,339 | 148,700 (security-reviewer opus ×3: CR (blocking: FTS optimize on every eviction, +140 ms per capture at 10k) → CR (blocking: 10-minute claim false under steady captures) → APPROVED) | 57,085 (2 rounds) | 422,124 |
| 403 Organization UI | task/403-organization-ui | builder | sonnet | done | 319,456 | 143,609 (reviewer opus ×2: CR (blocking: dialog keys never reached the host, also Task 404 edit panel; D-052 dialog state) → APPROVED) | 16,574 | 479,639 |
| 405 Backup / export / import | task/405-backup-export | specialist | opus | done | 455,716 | 249,327 (security-reviewer opus ×3: CR (blocking: UTF-16 backup broke the store; NoCloud clips exported) → CR (blocking: hidden `sqlite_*` trigger survived the schema check; pending copy read after a queued Private) → APPROVED) | 150,115 (2 rounds) | 855,158 |
| 406 Fuzzing | task/406-fuzzing | specialist | opus | done (stable fallback harness; libFuzzer not run, no nightly) | 295,069 | 98,678 (security-reviewer opus: APPROVED with 2 should-fix (secret-shaped seed corpus, decisions) → fixed) | 16,219 | 409,966 |
| 412 Update check | task/412-update-check | builder | sonnet | done | 179,398 | 112,645 (security-reviewer opus: APPROVED with 5 should-fix (shown tag metadata, gate gaps, licence notice, docs, threat model) → fixed, verified by orchestrator gate) | 33,309 | 325,352 |
| 407 Security re-review | task/407-security-review | specialist (as reviewer; needs to write the report) | opus | done: 1 high (WER crash dumps), 4 medium → Task 413 | 216,790 | (orchestrator) | | 216,790 |
| 413 Release blockers (407 findings) | task/413-release-blockers | specialist | opus | done | 319,940 | 217,332 (security-reviewer opus ×2: CR (blocking: DACL could reach a user-chosen folder with other files) → APPROVED; CI confirmed the elevated-runner owner rule) | 44,656 | 581,928 |
| 409 Licence | — | — | — | done in Task 201 (D-045) | | | | |
| 408 Release workflow | task/408-release-workflow | builder | sonnet | done (workflow first runs after merge: dispatch dry run) | 83,653 | 108,640 (reviewer opus ×2: CR (blocking: release.yml invalid YAML) → APPROVED) | 12,141 | 204,434 |
| 410 0.1.0 docs and version | task/410-docs-0.1.0 | scaffolder | sonnet | done | 103,856 | 115,623 (reviewer opus ×2: CR (blocking: Known issues listed under Added in the release body; unbacked claims) → APPROVED) | 10,855 | 230,334 |
| Hotfix release-notes exit code | fix/release-notes-exit (PR #32) | orchestrator | opus | done (first release dry run failed on a leaked `$LASTEXITCODE`) | not measured | | | |
| Hotfix notices fetch | fix/notices-fetch (PR #34) | orchestrator | opus | done (second dry run: offline cargo-about needed all locked crates) | not measured | | | |
| **Release 0.1.0** | tags `v0.1.0-rc.1`, `v0.1.0` | orchestrator | — | **released 2026-11-24** (D-069) | | | | |
| **Release 0.1.1** (Task 414 fixes; owner approved, D-070) | tag `v0.1.1` | orchestrator | — | **released 2026-11-24** (D-071) | | | | |
| 414 Icons without font glyphs; popup footer fits (owner report on 0.1.0) | task/414-icon-glyphs | builder | sonnet | done | 111,576 | 41,080 (reviewer sonnet: APPROVED, 1 should-fix (icon centring) → fixed) | 19,722 | 172,378 |
| Phase 5 review | — | specialist | opus | **CHANGES REQUIRED** (D-072): 1 blocking (B1: backup snapshot skips FTS merge/VACUUM unless masked/expired clips were purged, so evicted clips' and replaced notes' words can leave in a `.dcbackup`); 7 non-blocking. Gate: ALL CHECKS PASSED (1000 tests, 0 failed, 32 ignored) | 162,582 | | | 162,582 |
| 415 Phase 5 review fixes | task/415-phase5-review-fixes | specialist | opus | done | 101,451 | 77,853 (security-reviewer opus ×2: CR (blocking: the now-unconditional snapshot VACUUM ran without temp_store=MEMORY; this SQLite is built TEMP_STORE=1, so >2 MiB histories spill a copy to %TEMP%) → APPROVED) | 11,441 | 190,745 |
| Phase 5 re-review | — | specialist | opus | **APPROVED** (D-074); gate ALL CHECKS PASSED (1006 passed, 0 failed, 32 ignored). **Phase 5 complete** | 19,518 | | | 19,518 |
| 416 temp_store=MEMORY on every connection; migration copy merged (F-29, F-30) | task/416-temp-store-memory | specialist | opus | done | 87,219 | 46,556 (security-reviewer opus: APPROVED, 2 nits) | | 133,775 |
| **Release 0.1.2** (B1 + F-29/F-30; owner approved, D-075) | tag `v0.1.2` | orchestrator | — | **released 2026-11-24** (D-076) | | | | |
| 417 Gate rule: SQLite connections keep temp data in memory (D-077 item 7) | task/417-sqlite-connection-gate | builder | sonnet | done (window 25 lines bounded by the next `fn`, so `db.rs` passes unchanged) | 73,426 | 35,075 (reviewer sonnet: APPROVED, 1 should-fix (test-module skip hid the rest of the file) → fixed, verified by orchestrator gate) | 8,361 | 116,862 |

## Phase 7: UI redesign (approved 2026-11-24, D-078)

| Task | Branch | Agent | Model | Status | Tokens (impl) | Tokens (review) | Tokens (fix) | Total |
|---|---|---|---|---|---|---|---|---|
| Design package (audit agent sonnet 204,608; builder check sonnet 74,866) | docs/ui-redesign-design | orchestrator + general-purpose | opus + sonnet | done | 279,474 | | | 279,474 |
| 700 Foundation kit (tokens, SVG icons, components) | task/700-ui-foundation-kit | builder | sonnet | done (Lucide 1.79.0, 43 SVGs; P4 +0.12 MiB, P6 +36 KiB; D-079 menus) | 218,922 | 61,065 (reviewer sonnet: APPROVED, 3 should-fix: empty-menu guard fixed; key-map split and tooltip reset → F-31) | 2,862 | 282,849 |
| 710 App icon (option A) | task/710-app-icon | builder | sonnet | done (ICO 8 sizes 57 KB; tray RGBA per state via `nearest_tray_size`; `embed-resource` 3.0.12 Windows build-dep) | 124,033 | 41,771 (reviewer sonnet: APPROVED, nits only) | 0 | 165,804 |
| 701 Popup layout (package: scaffolder sonnet 140,626) | task/701-popup-layout | builder | sonnet | done (440×520, 48 px rows, row actions + kit row menu, filter segment + More filters, status line, floating toast; masked rows show only i18n text; F-31 closed) | 549,290 (incl. package) | 93,271 (reviewer opus: APPROVED, 1 should-fix → F-32) | 0 | 642,561 |
| 702 Popup preview pane (package: scaffolder sonnet 83,913) | task/702-popup-preview | builder | sonnet | done (side pane 760 px, pure `widen` placement, Overlay on narrow monitors, Esc closes preview first, facts; masked shows kind only) | 242,192 (incl. package) | 42,844 (reviewer sonnet: APPROVED, nits) | 0 | 285,036 |
| 703 History layout (package: scaffolder sonnet 100,719) | task/703-history-layout | builder | sonnet | done (sidebar groups, toolbar + date menu, 56 px rows, detail panel with toolbar and sections, status line; footer/org row/hints removed; masked rows and detail show no content; F-32 dead code removed; D-080, D-081) | 500,636 (incl. package) | 126,210 (reviewer opus: APPROVED, 4 should-fix: Favorites chip and collapsed-group title fixed, focus row added, size → 704) | 8,094 | 634,940 |
| 704 History interaction (package: scaffolder sonnet 76,077) | task/704-history-interaction | builder | sonnet | done (keyboard row menu in Rust with Export, bulk bar, select mode, Wide/Medium/Narrow layouts with rail and drawer, F6 regions, Ctrl+B/F/K/,, Esc layers, min 640×460 default 1060×660; F-34 closed; D-083) | 403,967 (incl. package) | 44,108 (reviewer sonnet: APPROVED, nits) | 5,526 | 453,601 |
| 705 Settings (package: scaffolder sonnet 83,008) | task/705-settings | builder | sonnet | done (five sections, kit Select/Field/ConfirmDialog, Always-protected card without controls, dirty save bar + Ctrl+S with edit counter, settings key map; D-082) | 357,438 (incl. package) | 77,983 (security-reviewer opus: APPROVED, 2 should-fix (dirty during save, Enter on Switch in dialog) fixed; focus return → 707) | 8,872 | 444,293 |
| 706 Secondary surfaces + F1 sheet (package: scaffolder sonnet 73,791) | task/706-secondary-surfaces | builder | sonnet | done (F1/Ctrl+/ shortcuts sheet with drift test against the real key maps, EmptyState, welcome, Copy as/Edit panels, org dialogs, delete confirmation restyled; D-084) | 379,484 (incl. package) | 34,284 (reviewer sonnet: APPROVED, nits) | 0 | 413,768 |
| 707 Accessibility pass (packages 707+708: scaffolder sonnet 74,543) | task/707-accessibility | builder | sonnet | done (A3 role/label scan over 73 instances, A6 hit-target scan, Narrator description tests incl. masked rows in both windows, A7 budgets for welcome/F1 sheet/dialogs, settings focus return to the opener; D-085; nothing needed fixing in labels/targets) | 234,312 (incl. packages) | 43,260 (reviewer sonnet: APPROVED, 1 should-fix → F-36) | 0 | 277,572 |
| 708 Verification | task/708-verification | platform-verifier | sonnet | done (v0.1.2 vs Phase 7 on 10k clips: P1 quiet 16.6 → 22.3 ms ≤ 30; P3 9.69 → 9.59 MiB (absolute 9 MiB missed by both, D-086); P4 +0.56 MiB; P5 0 %; P6 +951 KiB (budget 300, missed, F-39); P2 not measurable; demo checklist added) | 78,656 | 25,633 (reviewer sonnet: APPROVED) | 0 | 104,289 |
| 709 Phase review | main 4b69ad1 (read-only worktree) | specialist | opus | done: **APPROVED** (Review Focus 1–4, 6 proven in code; 5 = Task 708; nits → F-38) | | 128,253 | | 128,253 |
| 712 Exe size after Phase 7 (F-39, owner request) | task/712-exe-size | builder | sonnet | done (per-merge size table; PNG window icon pulled in Slint image decoders: now raw RGBA, -100 KiB; KitGallery removed (no size effect); remaining growth +851 KiB is generated Slint code for the new UI + required icon data; budget still missed, D-087) | 127,685 | 32,180 (reviewer sonnet: APPROVED) | 0 | 159,865 |

Phase 7 complete 2026-11-25 (709 APPROVED; budgets per D-086; every visual, keyboard, Narrator and DPI check is an untested owner row, see the "Phase 7 demo checklist" in `docs/testing/platform-checklist.md`).

## Totals by model

| Model | Tokens |
|---|---|
| opus | 12,518,058 |
| sonnet | 9,389,860 |
| haiku | 0 |
| **Total** | **21,907,918** (subagents only; orchestrator not measured) |
