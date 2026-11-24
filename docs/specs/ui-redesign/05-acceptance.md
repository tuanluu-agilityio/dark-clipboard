# UI redesign: accessibility and performance acceptance criteria

Status: **proposed**. These become acceptance criteria in the task packages (Phase 7 plan). "Auto" =
checked by a test or script in `scripts/check.ps1`; "Owner" = a row in
`docs/testing/platform-checklist.md` (GC-19: agents cannot click, type or take screenshots).

## 1. Accessibility

| # | Criterion | How |
|---|---|---|
| A1 | Every text token pair in use has contrast ≥ 4.5:1 (`text`, `text-2`, `text-3`, `accent`, `danger`, `success`, `warning` on `canvas`, `surface`, `selected`; `on-accent` on `accent`). `text-3` is not used on `raised`. Values recorded below. | Auto: extend the WCAG test in `src/theme.rs` with every pair |
| A2 | Focus ring (`focus` on its background) ≥ 3:1; selected row vs canvas is distinguishable by the accent bar (non-colour cue) | Auto (contrast) + Owner |
| A3 | Every interactive element has `accessible-role` and a non-empty `accessible-label` from i18n | Auto: a scan test over `ui/**/*.slint` (each `IconButton`, `NavItem`, `Switch`, menu item instance binds a label) |
| A4 | Every action is reachable by keyboard; Tab reaches every control; no keyboard trap; Esc unwinds one layer | Auto: key-map unit tests for new keys; Owner: Tab walk per window |
| A5 | Tooltips show on keyboard focus as well as hover | Owner |
| A6 | Hit targets ≥ 28 × 28 px (≥ 24 minimum) | Auto: scan test for `IconButton` size constants; Owner spot check |
| A7 | Vietnamese: no label in a fixed-width row is cut off at the window minimum sizes; width budget tests (as `tests/footer_budget.rs`) for: popup header + status line, history toolbar + bulk bar, settings nav | Auto (character budget) + Owner (EN and VI, 100/150/200 %) |
| A8 | Narrator reads: row label + description, button names, switch state, section names | Owner (Narrator) |
| A9 | Icons are never text glyphs; `tests/no_font_glyphs.rs` still passes | Auto |
| A10 | Meaning never relies on colour alone (masked: icon + italic + label; sensitive/danger: icon + text) | Review |

Computed contrast (proposed tokens, WCAG 2.x, 2026-11-24, Python script in the design session):

| Foreground | canvas | surface | raised | selected |
|---|---|---|---|---|
| text `#F3F5F9` | 17.02 | 14.99 | 13.36 | 12.59 |
| text-2 `#A1AAB8` | 7.93 | 6.98 | 6.22 | 5.86 |
| text-3 `#8F98A8` | 6.39 | 5.63 | 5.02 | 4.73 |
| accent `#7887FF` | 5.94 | 5.23 | 4.66 | 4.39 (not used as text on selected; icon only, ≥ 3:1) |
| success `#5FD4AA` | 10.16 | 8.95 | 7.98 | 7.51 |
| warning `#F2C14E` | 11.07 | 9.75 | 8.69 | 8.19 |
| danger `#FF7A85` | 7.41 | 6.52 | 5.82 | 5.48 |
| focus `#9AA6FF` | 8.20 | 7.22 | 6.44 | 6.07 |
| on-accent `#111318` on accent | 5.94 | | | |

Search highlight on a selected row uses `accent` + 600 weight on `selected` (4.39): **below 4.5**
for normal text. Decision: highlights use `text` colour + accent underline (2 px) on selected rows,
accent colour only on unselected rows (5.94). Fixes audit §6.1 failures (3.24–3.77).

## 2. Performance (baseline: Task 212 / 0.1.x on Windows, software renderer)

| # | Criterion | Baseline | Budget | How |
|---|---|---|---|---|
| P1 | Popup open (shortcut to first frame) p95 | 21.1 ms | ≤ 30 ms (and NFR budget 100 ms) | Existing `perf` log timing, platform-verifier run, 3 runs |
| P2 | Search keystroke to list update p95 | `perf what=search_done` (F-21) | no regression > 10 % | same |
| P3 | Memory, app hidden after start | 6.9 MiB private | ≤ 9 MiB | Existing measurement script (Task 212 method) |
| P4 | Memory after opening popup, history, settings once | measure at Task 700 start | ≤ baseline + 4 MiB | same |
| P5 | Idle CPU (hidden, 60 s) | 0 % | 0 % | same |
| P6 | Binary size | measure at Task 700 start | ≤ + 300 KiB (≈ 40 small SVGs) | `ls` of release exe |
| P7 | No animation longer than 120 ms; no timer runs while windows are hidden | — | — | Review + idle CPU |
| P8 | List scrolling with 10k clips stays smooth (no per-row image work: kind icons are shared images) | — | — | Owner + review |

Expected impact: SVG icons are parsed once and cached as images by Slint; a 16 px icon rasterised
at 200 % is 32 × 32 × 4 B = 4 KiB, ~40 icons ≈ 160 KiB in the worst case. Rows get simpler (one
icon instead of a text tile; fewer `Text` elements in the footer). Two-pane popup adds one
`DetailPane` that already exists. Net: expected flat CPU, memory within P3/P4, to be measured,
not assumed.

## 3. Behaviour preserved (regression criteria)

All existing tests pass unchanged except where a task package says a test changes because a
visible text moved (named per task). Specifically kept: masked clips never load content; paste only
on Ctrl+Enter (or the setting); undo timing; capture-off until Start in welcome; exclusion-marker
row non-interactive (GC-07); every string from i18n (EN + VI); popup placement keeps the window on
one monitor; history window size remembered per monitor.

## 4. Manual checklist additions (owner rows, one per redesign task)

Per window: renders at 100/150/200 % and on a mixed-DPI two-monitor setup; Tab walk; every shortcut
in `04-interaction-keyboard.md`; Narrator pass; Vietnamese pass; masked clip shows no content;
P1/P3 measured by the platform-verifier agent where it can (headless timing), the rest by the owner.
