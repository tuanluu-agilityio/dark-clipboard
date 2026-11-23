# Progress and token ledger

Updated: 2026-11-23 (Phase 3 complete)

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
| 411 Data management | task/411-data-management | builder | sonnet | in progress | | | | |
| 403 Organization UI | task/403-organization-ui | builder | sonnet | pending | | | | |
| 405 Backup / export / import | task/405-backup-export | specialist | opus | pending | | | | |
| 406 Fuzzing | task/406-fuzzing | specialist | opus | pending | | | | |
| 412 Update check | task/412-update-check | builder | sonnet | pending | | | | |
| 407 Security re-review | task/407-security-review | security-reviewer | opus | pending | | | | |
| 409 Licence | — | — | — | done in Task 201 (D-045) | | | | |
| 408, 410 Release workflow, 0.1.0 docs | — | — | — | pending (owner authorized releases and tags, D-058) | | | | |

## Totals by model

| Model | Tokens |
|---|---|
| opus | 8,892,662 |
| sonnet | 4,221,366 |
| haiku | 0 |
| **Total** | **13,114,028** (subagents only; orchestrator not measured) |
