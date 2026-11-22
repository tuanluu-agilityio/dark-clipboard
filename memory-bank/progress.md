# Progress and token ledger

Updated: 2026-11-22 (Phase 2 started)

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
| 107 Windows verification | task/107-windows-verification | platform-verifier | sonnet | next (waiting for the owner at the machine) | | | | |
| 108 Phase review | main | specialist | opus | pending | | | | |

## Totals by model

| Model | Tokens |
|---|---|
| opus | 4,644,739 |
| sonnet | 2,112,098 |
| haiku | 0 |
| **Total** | **6,756,837** (subagents only; orchestrator not measured) |
