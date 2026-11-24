# UI audit (baseline for the redesign)

Scope: `apps/desktop/ui/*.slint` (Slint 1.19, software renderer, always dark), hosts in `apps/desktop/src/ui/`, key maps in `popup/keys.rs` and `history/keys.rs`. Paths below are relative to `apps/desktop/` unless they start with `crates/` or `docs/`. Everything is read from source; nothing was run, rendered or measured (GC-19). "Not verified" marks claims that need a render or a runtime check. Line numbers are of the worktree at the time of writing.

## 1. Inventory

Only `app.slint` is compiled by `build.rs:4` (it re-exports every other component, `app.slint:12`). All strings arrive through the globals `Strings`, `OrgStrings`, `SText`, `WText` (`ui/theme.slint:18`, `ui/organize.slint:11`, `ui/settings.slint:25`, `ui/welcome.slint:9`), filled from Rust (`src/ui/mod.rs:127-141`, `src/ui/common.rs:260`).

### 1.1 Windows

| Window | Component | Size | Layout (top to bottom) | Host |
|---|---|---|---|---|
| Popup | `MainWindow` `ui/app.slint:17` | 420 x 560 fixed, frameless, always-on-top, transparent (`app.slint:19-23`); size also hard-coded in Rust `src/popup/place.rs:5` | Rounded panel (r10, 1px line border, `app.slint:112-117`), padding 10, spacing 8: search row 36 px (field + gear + close, `:124-187`), chips row 28 px sideways-scrolling Flickable (`:190-216`), list or state text (`:219-268`), optional detail pane 220 px (`:271-281`), 1px rule (`:284`), footer 28 px: 5 text buttons or toast + Undo (`:288-352`) | `src/ui/popup_host.rs` |
| History | `HistoryWindow` `ui/history.slint:111` | min 760x460, preferred 980x640 (`:114-117`); Rust mirrors in `src/history/place.rs:11,13`; size remembered per monitor | padding 12, spacing 10: search 36 (`:263-301`); 3 columns (`:303-478`): filters 180 px (`:308-359`), list stretch 3 / min 300 (`:367-423`), detail+org strip stretch 2 / min 240 (`:431-477`); rule; footer 28 px with up to 8 text buttons + toast/summary (`:486-554`); org row 26 px with up to 5 text buttons (`:556-594`); three 11 px hint lines (`:595-615`) | `src/ui/history_host.rs`, `history_host/org.rs` |
| Settings | `SettingsWindow` `ui/settings.slint:177` | min 640x480, preferred 780x600 (`:180-183`) | Left sidebar 170 px with 4 `SectionTab` (`:299-336`); right: optional banner (`:342-357`), ScrollView with padding 18 / spacing 10 and the section (`:359-892`), Save bar 52 px (`:895-925`, hidden on About); five modal `Overlay`s (`:930-1190`) | `src/ui/settings_host.rs`, `src/prefs/*` |
| Welcome | `WelcomeWindow` `ui/welcome.slint:23` | min 480x420, preferred 520x460 (`:26-29`) | One column, padding 24 / spacing 12: title 20 px, 4 body paragraphs, optional notes, `CheckBox`, `Button`s right-aligned (`:63-149`) | `src/ui/welcome_host.rs` |
| Tray menu, start-failure box | native (`src/tray.rs`, `src/start_failure.rs`) | n/a | Not Slint; out of scope except that labels come from i18n | `src/ui/tray_host.rs` |

### 1.2 Components (current library)

| Component | File:line | Notes |
|---|---|---|
| `Theme`, `Strings` globals | `ui/theme.slint:3,18` | Colours are `in` properties set from `src/theme.rs:30-45` (single source of truth) |
| `Icon`, `IconKind {gear, close, star, lock}`, one `Path` with a `commands:` ternary per kind | `ui/icons.slint:7-31` | 24x24 viewbox; close is stroked (2.6), others filled (`:24-26`) |
| `ChipView` (FocusScope, checkbox role) | `ui/popup.slint:70-123` | 26 px high, r13, text 12 px, focus = 2px `Theme.text` border (`:106-107`) |
| `ActionButton` (text only) | `ui/popup.slint:125-155` | 26 px high, r5, text 12 px, hover bg `surface`; not a FocusScope |
| `IconButton` (icon only) | `ui/popup.slint:157-186` | Used twice in the whole app (popup gear and close, `app.slint:168-186`); icon 16 px; not focusable; no tooltip |
| `Badge` | `ui/popup.slint:189-204` | 15 px high, 10 px text, border + text in tint |
| `RowView` | `ui/popup.slint:206-343` | 68 px (`RowMetrics`, `:66-68`): pin bar, 30 px kind tile with 2-3 letter text, two 18 px preview lines of `Seg`s, 16 px meta line |
| `DetailPane` | `ui/popup.slint:347-473` | kinds 0 text, 1 image, 2 files, 3 masked; text-only "Load full text" / "Load larger preview" buttons (`:376-387`) |
| `TransformPanel`, `EditPanel`, `TfItem` | `ui/transform.slint:40,163,7` | Modal overlays, scrim `#000000a0`, card 600x460 / 720x520 (`:59-60,177-178`) |
| `OrgStrip`, `OrgDialog` | `ui/organize.slint:49,177` | Strip: favorite, note, tags as plain rows; dialog serves note, tags, collection name, picker |
| `FilterButton`, `MenuEntry` | `ui/history.slint:21,76` | Left filter row 28 px; context-menu row 30 px (not focusable) |
| `SectionTab`, `Overlay`, `Caption`, `FieldError` | `ui/settings.slint:137,168,122,129` | Settings only. Form controls are stock `std-widgets` `Button`, `CheckBox`, `ComboBox`, `LineEdit`, `ListView` (`:9`); style not set in repo (no `SLINT_STYLE` or `with_style` found), so the style is Slint's default: not verified which |

### 1.3 Interaction contract (properties and callbacks the redesign must keep or consciously replace)

Popup `MainWindow` (`ui/app.slint:26-74`), bound in `src/ui/popup_host.rs:1046-1158`:

| Kind | Names |
|---|---|
| in | `rows [ClipRow]`, `chips [Chip]`, `selected`, `view-state` (0 rows, 1 loading, 2 empty, 3 no results, 4 error), `toast`, `toast-undo`, `selected-pinned`, `has-selection`, `detail-open`, `detail DetailState`, `detail-text`, `can-transform`, `tf-open/-labels/-selected/-preview/-note/-note-error/-can-copy`; in-out `query`; out `page-rows` (uses `RowMetrics.height`) |
| callbacks | `query-edited`, `key(text, ctrl, shift, alt, meta, search-focused) -> bool` (capture phase, `app.slint:105-110`), `chip-focus`, `chip-toggled`, `row-clicked`, `row-activated`, `act-copy`, `act-copy-plain`, `act-copy-as`, `tf-chosen`, `tf-copy`, `tf-cancel`, `act-pin`, `act-delete`, `act-undo`, `act-close`, `act-settings`, `load-full-text`, `load-full-image`, `shown-until` |
| functions | `focus-search()`, `scroll-to(i)` (`app.slint:76-91`); the host calls both |

History `HistoryWindow` (`ui/history.slint:120-214`), bound in `src/ui/history_host.rs:982-1123` and `history_host/org.rs:963-1002`:

| Kind | Names |
|---|---|
| in | `rows`, `filters [FilterItem]`, `selected`, `view-state`, `toast`, `has-selection`, `can-copy`, `will-unpin`, `summary`, `detail-open/detail/detail-text`, `confirm-text/-label/-hint`, `can-transform`, `tf-*`, `edit-open/-note`, `org-*` (strip, favorite, fav-text, can-favorite/add/remove/edit, note, tags), `col-shown`, `dlg-open`, `dlg OrgDialogState`; in-out `edit-text`, `query`, `dlg-text`; out `page-rows` |
| callbacks | `query-edited`, `key(...)`, `filter-focus`, `filter-toggled`, `row-pressed(i, ctrl, shift)`, `row-context(i)`, `row-activated`, `act-copy/-copy-plain/-copy-as/-edit/-pin/-delete/-select-all/-clear-selection/-confirm/-cancel`, `tf-chosen/-copy/-cancel`, `edit-save/-cancel`, `load-full-text/-image`, `shown-until`, `act-favorite/-note/-tags/-add-collection/-remove-collection/-new-collection/-rename-collection/-delete-collection`, `org-remove-tag(i)`, `dlg-edited/-submit/-cancel/-choose/-remove-chip` |
| functions | `focus-search()`, `scroll-to(i)`, `open-menu-at-selected()` (`history.slint:216-237`) |

Settings `SettingsWindow` (`ui/settings.slint:187-282`), bound in `src/ui/settings_host.rs:1217-1347`: in-out form values (`shortcut`, `lang-index`, `launch-at-login`, `position-index`, `close-on-focus-loss`, `enter-pastes`, `sensitive-index`, `kinds [KindItem]`, `max-items/-age/-storage`, `new-app`, `older-days`, `clear-keep-pinned`, `auto-update`), in-only state (`section`, `can-save`, `apps [AppRow]`, `err-*`, `status/status-error`, `mode`, overlay flags, `stats-text`, `skip-text`, `caps-text`, `version-text`, `update-*`, `data-path`), 36 callbacks (`save`, `revert`, `edited`, `set-mode(int)`, `add-app/remove-app/open-picker/pick-app`, `clear-*`, `cleanup-*`, `restore-*`, `limits-*`, `backup-ask/restore-ask/import-ask`, `open-folder`, `show-welcome`, `check-updates`, `auto-update-toggled`, `open-release`, `start-capturing`, `escape`).

Welcome (`ui/welcome.slint:33-48`, `welcome_host.rs:127-129`): `login` (in-out), `can-login`, `login-enabled`, `show-capture-off`, `error`, `busy`, `finished`; callbacks `begin`, `not-now`, `dismiss`.

Shared structs: `ClipRow`, `Seg`, `FileRow`, `DetailState`, `Chip` (`ui/popup.slint:9-64`), `FilterItem` (`history.slint:13`), `OrgDialogState` (`organize.slint:29`), `AppRow`, `KindItem` (`settings.slint:13,20`). Row content is built in Rust (`src/ui/common.rs:27-62`, `src/popup/row.rs`), so icon choice per clip kind must be added there.

### 1.4 Transform, edit, organise dialogs, toast

| Piece | Where | Behaviour |
|---|---|---|
| Copy as | `transform.slint:40-160` | Scrim over the whole window; list of transform names (170 px, `TfItem` 28 px) + preview + note + hint + Cancel/Copy. Keys: Up/Down/Enter/Esc, all others swallowed (`src/popup/transform.rs:26-33`, `popup_host.rs:1067-1071`) |
| Edit copy (history only) | `transform.slint:163-255`, host `history_host.rs:1005-1014` | Esc cancels, Ctrl+Enter saves, rest to the text box |
| Delete confirm (history) | `history.slint:740-784` | Modal; Enter confirms, Esc cancels, other keys swallowed (`history/keys.rs:69-75`) |
| Org dialog | `organize.slint:177-403`, keys `organize/dialog.rs:61-90`, `history_host/org.rs:392-426` | Enter submits (Note: Ctrl+Enter), Esc cancels, Up/Down in pickers, Backspace on empty tag box removes last chip |
| Popup toast + Undo | `app.slint:333-352`; timers `popup_host.rs:56,58` | 5 s with Undo, 4 s for errors. The toast replaces the five footer buttons while visible |
| History toast | `history.slint:545-553` | Right-aligned text in the footer row, same row as 8 buttons |
| Settings overlays | `settings.slint:930-1190` | Five near-identical blocks (remove built-in exclusion, limits, clear, cleanup, restore) plus the running-app picker |

## 2. Current design tokens

Colours live in one place (`ui/theme.slint:4-12`, `src/theme.rs:11-19`): bg `#1C1E22`, surface `#24272C`, line `#383C43`, text `#E7E9EC`, text-muted `#A3A9B2`, accent `#4C8DFF`, danger `#FF7B72`, success `#4ADE80`, warning `#F5C04A`. Everything else is a literal sprinkled through the markup. Counts below are `grep` counts over `ui/*.slint` (declarations, not instances at runtime).

| Token class | Distinct literals found (count) | Observation |
|---|---|---|
| Colour literals outside Theme | scrim `#000000a0` / `#000000A0` x4 (`transform.slint:52,170`, `organize.slint:186`, `history.slint:741`, `settings.slint:171`) | Same scrim, four spellings |
| Alpha tints | `accent.with-alpha`: 0.25 x3 (`history.slint:52`, `organize.slint:323`, `settings.slint:143`), 0.34 / 0.22 (`popup.slint:219`); `surface.with-alpha(0.5)` hover (`:221`); `warning.with-alpha` 0.18 x2 (`settings.slint:343,474`); `text-muted.with-alpha(0.6)` (`history.slint:97`) | 6 ad-hoc alpha values |
| Font sizes | 11 px x17, 12 px x15, 13 px x10, 14 px x7, 10 px x2, 20 px, 18 px; `default-font-size: 13px` on four windows; settings body text inherits 13 | 7 sizes, no scale names; 10-11 px used for meta, hints, headings of groups, badges, file paths |
| Font weights | 700 x18, conditional 700/400 x3 | Bold used as the only emphasis |
| Radii | 6 px x17, 8 px x10, 5 px x3, 4, 7.5, 10, 13, 1.5 | 8 values; buttons 5, chips 13, rows 6, cards 8, popup 10, inputs 6, menu 6 |
| Spacing | 8 px x26, 10 px x9, 6 px x8, 4 px x8, 12 px x6, 2 px x3, 0, 1, 16 | 9 values |
| Padding | 14 px x6, 10 px x5 (+3+3 L/R), 12 px x4, 8 px x2, 24, 18, 16, 1, 6 | Card padding is 12 (transform, org dialog), 14 (settings overlays), 16 (history confirm), 24 (welcome) |
| Border widths | 1 px x21; focus rings 2 px (chip, filter) or 1 px accent (list, search) | Focus treatment differs per component |
| Heights (controls) | 30 x9, 28 x6, 26 x6, 36 x5, 34 x2, 24 x2, 22, 18, 16, 15, 13-14 | Button 26, chip 26, filter 28, menu 30, tab 36, search 36, settings app row 34 |
| Row height | 68 px (`popup.slint:67`) shared by popup and history; kind tile 30; preview lines 18; meta 16; pin bar 3 px | Single metric, good |
| Widths | popup 420, history pane 180 / min 300 / min 240, sidebar 170, menu 230, dialogs 420 / 480 / 600 / 720 | Dialog widths differ per file |
| Icons | 16 px (`IconButton`), 14 px (lock, org star), 13 px (row star) | 4 icon kinds only |

## 3. Keyboard map today

"S" = search box focused, "L" = list owns keys (search empty or focus elsewhere). A focused chip or filter keeps Enter/Space/arrows and passes only Esc (`popup/keys.rs:161-163`, `history/keys.rs:76-78`).

### 3.1 Popup (`src/popup/keys.rs:160-203`, dispatch `popup_host.rs:846-896`, hook `:1065-1091`)

| Key | Action | Note |
|---|---|---|
| Up / Down | move selection | always (`keys.rs:168-169`) |
| PgUp / PgDn | move by `page-rows` | always (`:174-175`) |
| Home / End | first / last row | L only; Ctrl+Home/End always (`:176-177`) |
| Enter | copy original (or paste when "Enter pastes") | `:181`, paste rules `popup/paste.rs:62-68` |
| Shift+Enter | copy plain text | `:180` |
| Ctrl+Enter | copy, return focus, paste | `:179` |
| Alt+1..9 | copy n-th visible row | `:198-201`; badge shown in each row's meta (`row.rs:92-96`) |
| Esc | return focus and close | `:182`, `popup_host.rs:884-897` |
| Del | delete selected (5 s undo) | L only (`:183`) |
| Ctrl+Z | undo delete | only while one is pending (`:195`) |
| Ctrl+P | pin / unpin | `:184` |
| Ctrl+D | favorite / unfavorite | `:186` |
| Ctrl+T | Copy as... | `:185` |
| Ctrl+H | open history window | `:191`; also arrives as Ctrl+Backspace code (`:192`) |
| Right / Left / Space | open / close / toggle detail pane | L only (`:171-173`) |
| Tab / Shift+Tab | Slint navigation search, chips, list | `keys` scope has `focus-on-tab-navigation: false` (`app.slint:104`) |
| Chip focused: Enter / Space | toggle chip | `popup.slint:88-94` |
| "Copy as" panel open | Up/Down choose, Enter copy, Esc close, everything else swallowed | `popup/transform.rs:26-33` |

No key exists for Settings (gear button only, `app.slint:168-176`).

### 3.2 History (`src/history/keys.rs:68-130`, dispatch `history_host.rs:700-769`, hook `:997-1038`)

| Key | Action | Line |
|---|---|---|
| Up/Down/PgUp/PgDn | move (resets marks to the cursor) | `keys.rs:100` |
| Shift + those, Shift+Home/End | extend marked range | `:100,102` |
| Ctrl + those, Ctrl+Home/End | move cursor, keep marks | `:103-105` |
| Home / End | first / last | L only (`:102`) |
| Space | mark / unmark row | L only (`:106`) |
| Enter / Shift+Enter | copy / copy plain (no Ctrl+Enter paste) | `:107-109` |
| Esc | clear marks, else close | `:110`, `history_host.rs:744-754` |
| Del | delete (opens confirm) | L only (`:111`) |
| Menu key | context menu at selected row | `:112` |
| Ctrl+P / T / E / D / N / G | pin / Copy as / Edit / favorite / note / tags | `:113-118` |
| Ctrl+L / Ctrl+Shift+L | add to / remove from collection | `:119-120` |
| Ctrl+Shift+N / R / Del / E | new / rename / delete collection / export | `:121-124` |
| Ctrl+A | select all rows | L only (`:126`) |
| Confirm dialog | Enter confirm, Esc cancel, all else swallowed | `:69-75` |
| Copy-as panel, edit panel, org dialog | as in 1.4 | `history_host.rs:1001-1019` |

### 3.3 Settings and welcome

| Window | Key | Action | Line |
|---|---|---|---|
| Settings | Esc | close the open overlay, else hide window | `settings.slint:288-295`, `settings_host.rs:1347-1363` |
| Settings | Tab, Space, Enter, arrows | stock widget behaviour only; no accelerator for Save, no section shortcut | not verified at runtime |
| Welcome | Esc | dismiss (same as Not now) | `welcome.slint:54-61` |
| Welcome | Enter | no handler (relies on the focused `Button`) | `welcome.slint:54-61` |

## 4. Text versus icon

### 4.1 Text-only controls that could carry an icon

| Control | Where | Text (EN) | Could be |
|---|---|---|---|
| Popup footer: Copy, Plain text, Copy as..., Pin/Unpin, Delete | `app.slint:292-328` | 5 words, 369 px of 398 in Vietnamese by the test's own estimate (see 7) | icon buttons with tooltip; Pin toggles icon state |
| Popup detail: "Load full text", "Load larger preview" | `popup.slint:376-387` | sentences | icon + short label |
| Popup toast Undo | `app.slint:345-351` | "Undo" | icon + label |
| History footer: Copy, Copy as plain text, Copy as..., Edit..., Pin/Unpin, Delete, Select all, Clear selection | `history.slint:490-544` | 8 buttons | icon toolbar |
| History org row: favorite, Add to collection..., Remove from collection..., Edit tags..., Edit note... | `history.slint:559-593` | long labels | star / folder / tag / note icons |
| History left pane: New / Rename / Delete collection | `history.slint:339-358` | full-width text buttons | `+` / pencil / trash icons |
| Context menu (12 entries, no icons, no shortcut column) | `history.slint:638-733` | text | icon + label + key hint |
| Org strip: Edit note..., Edit tags..., favorite text | `organize.slint:92-125,161-171` | text | icons |
| Tag and chip removal | `organize.slint:152-159,230-236` | literal "\u{00d7}" text button | `close` icon (the only symbol glyph in the markup; outside the scanned range, `no_font_glyphs.rs:19`) |
| Dialog buttons Cancel / Submit | `transform.slint:144-156,240-251`, `organize.slint:387-399`, `history.slint:769-780` | text | fine as text; primary one needs distinct style (today `ActionButton` has no primary variant) |
| Settings Save / Revert, capture mode (3 buttons), Add, Remove, Pick running, Clear, Delete old, Remove orphaned, Compact, Backup, Restore, Import, Open folder, Show welcome, Check updates | `settings.slint` 448-468, 600-628, 705-777, 817-855, 909-921 | stock `Button`s | segmented control for mode; icon+text for actions |
| Filter chips (popup) and filter rows (history) | `popup.slint:70-123`, `history.slint:21-73` | kind names ("Text", "Rich text", "Link"...) | kind icon + count or tooltip |
| Welcome: Not now / Start | `welcome.slint:126-148` | stock buttons | fine |

### 4.2 Symbols and glyphs in use

| Use | Where |
|---|---|
| Vector icons: gear, close (popup header), star (favorite in row, org strip), lock (settings markers) | `icons.slint:7-31`; `app.slint:168-186`, `popup.slint:304-312`, `organize.slint:81-91`, `settings.slint:516-527` |
| Kind tile: letters "Aa", "Rt", "://", "#", "</>", "Img", "Fi" in a 30 px tile, 11 px bold | `src/popup/row.rs:29-38`, drawn `popup.slint:251-265` |
| Text separators `·` (U+00B7) in meta, hints and toasts; arrow `→` (U+2192, allow-listed) in key hints; em dash in `UiPasteManual`; `...` in labels | `src/popup/row.rs:87-95`; `crates/darkclip-core/src/i18n.rs:139-151,152` |
| Pinned and masked badges are text pills ("Pinned"/"Đã ghim", "Masked"/"Đã che") | `popup.slint:189-204,319-322` |
| Pinned row: 3 px accent bar | `popup.slint:233-241` |
| Selected, marked, hover: background tints only | `popup.slint:219-221` |
| No tooltips anywhere (`grep -i tooltip ui/*.slint` is empty); `IconButton` relies on accessible label only | `popup.slint:157-186` |

### 4.3 Key-hint text

| Where | String | Visible? |
|---|---|---|
| Popup, 3 variants (copy only / Ctrl+Enter paste / Enter pastes) | `UiKeyHints*` `i18n.rs:139-151`, chosen in `popup/paste.rs:62-68` | No: only the search box's `accessible-description` (`app.slint:149`). The popup has no visible hint line |
| History hint line 1 | `Strings.key-hints` = `UiKeyHintsHistory` (`history_host.rs:119`, `i18n.rs:197`) or the confirm hint, `history.slint:596` | Yes, 11 px, elided |
| History hint line 2 | `OrgStrings.org-hints` = `OrgKeyHintsHistory` (about 150 chars EN) `i18n.rs:752-755` | Yes, 11 px, elided |
| History hint line 3 | `OrgStrings.collection-keys` `i18n.rs:829-832` | Yes, 11 px, elided |
| Copy as panel | `Strings.tf-hint` `transform.slint:132-137` | Yes |
| Edit panel | `Strings.edit-hint` `transform.slint:188-194` | Yes |
| Org dialog | `state.hint` `organize.slint:375-381` | Yes |
| Per-row `Alt+N` prefix in meta | `row.rs:92-96` | Yes, first 9 rows |

Note: the history hint lines mention Ctrl+A but not Ctrl+E, Ctrl+T, Space-mark, Ctrl+Shift+Del shown only in line 3; Ctrl+T and Ctrl+E (Copy as, Edit) appear in no hint line (checked `i18n.rs:197-200,752-755,829-832`).

## 5. UX problems

1. **Selection is almost invisible.** Selected row bg is `Theme.surface` (`popup.slint:220`) on `Theme.bg`: contrast 1.11:1. Hover is `surface` at 50 % (about 1.05:1, `:221`). The list border (1 px accent when the list has focus, `app.slint:229-233`) never shows while the search box keeps focus, which is the default state. Only the 3 px accent bar (pinned) and the marked tint stand out.
2. **Two sets of controls for the same actions.** Popup footer, history footer, context menu and org row repeat actions with different labels ("Copy as plain text" in history, "Plain text" in popup, `app.slint:300` vs `history.slint:498`) and different visibility rules.
3. **History footer is crowded.** 8 text buttons plus toast/summary in one 28 px row (`history.slint:486-554`), then another 26 px row of 5 more buttons (`:556-594`), then three 11 px hint lines (`:595-615`): 6 lines of chrome under the list. Estimated Vietnamese width of the footer with one clip selected is 717 of 736 px at minimum window width (7 px/char as in `tests/footer_budget.rs:16`; estimate, not rendered), leaving about 19 px for the toast/summary, which is elided.
4. **Popup chip row overflow.** 7 kind chips + Pinned + Today + 7 days + Favorites/collections/source chips (`popup/chips.rs:11-27,77-96`, `organize/nav.rs`) sit in a 28 px sideways scroller. Most chips are off-screen at 420 px; scrolling is by Tab-focus only (`app.slint:204-210`), no wheel or drag handling is declared (not verified).
5. **Detail pane eats the popup.** Fixed 220 px (`app.slint:272`) in a 560 px window. By arithmetic (padding 20, search 36, chips 28, rule 1, footer 28, spacing 8 per gap) the list drops from about 6 rows to 2 rows (68 px each) when open (derived, not rendered). Detail has no mouse toggle: only Right / Space / Left (`keys.rs:171-173`).
6. **Keyboard-only features without a visible control (popup):** favorite (Ctrl+D), detail pane, history (Ctrl+H, no button), Undo of a delete after the toast expires, Alt+1..9 (only discoverable through the `Alt+N` prefix in the meta line). **Mouse-only gaps:** history context menu entries are not keyboard-focusable (`docs/testing/platform-checklist.md:137`); `ActionButton`, `IconButton`, `MenuEntry`, `TfItem`, `SectionTab` are `Rectangle`+`TouchArea` with no `FocusScope`, so Tab never reaches footer buttons, gear, close, settings sidebar tabs or the org strip buttons (settings sidebar tabs: not verified at runtime; settings form widgets are stock and focusable).
7. **No visible hint line in the popup** (see 4.3) even though hints exist in 3 variants; users learn keys only from the per-row `Alt+N`. History has 3 hint lines of up to ~150 characters each, elided at 11 px (`history.slint:595-615`), so the end of the line (the useful rare shortcuts) is cut in both languages.
8. **Toast replaces actions.** While a toast shows (4-5 s) the five footer actions disappear (`app.slint:288-352`), so the selected clip cannot be pinned or deleted until it ends; the same toast row is where Undo lives.
9. **Row shows `Alt+N` in the history window too.** History reuses `clip_row` (`history_host.rs:155`, `common.rs:27`), so rows read `Alt+1 · ...` although the history key map has no Alt+digit action (`history/keys.rs:99-130`). Misleading hint.
10. **Row density.** A row carries the kind tile, two preview lines (44 or 38 chars hard-split in Rust, `row.rs:11-13`, so the cut does not follow the actual width), then a meta line `Alt+1 · Text · Notepad · 5 min ago` plus star, pin badge and masked badge squeezed into the same 16 px line (`popup.slint:291-322`). The kind name is repeated in meta although the tile already shows a code ("Aa", "Img"). The tile codes are abbreviations, not icons, and are not translated or read out (decoration).
11. **Focus visibility is inconsistent.** Chip focus: 2 px `Theme.text` border (`popup.slint:103-104`); filter row: 2 px `Theme.text` (`history.slint:53-54`); search: 1 px accent border colour change (`app.slint:133`); list: 1 px accent (`app.slint:231-232`); stock settings widgets: Slint style default. Buttons built on `ActionButton` have no focus state at all.
12. **Settings is a single long form.** General section: 12 stacked controls with captions (`settings.slint:367-437`). Privacy has 19 children in one scroll, including 7+ kind check boxes and an app list (`:440-635`). History section mixes retention fields, stats, destructive actions and backup/restore (`:638-781`). About mixes version, capabilities, data folder, updates and licences (`:784-890`). Save is explicit and global; fields are validated on Save with per-field errors; a pending change has no dirty indicator (the Save bar shows only a status line).
13. **Five duplicated overlay blocks in settings** (`:930-1190`) with the same structure, three different border colours (warning, danger, line) and padding 14.
14. **Dialog chrome varies.** Delete confirm: card r8, padding 16, 14 px text (`history.slint:747-783`); org dialog: padding 12, title 14 bold, up to 5 text lines of help (`organize.slint:200-401`); settings: padding 14, no title.
15. **Welcome uses stock `Button`/`CheckBox`** while the rest of the app uses custom controls; five paragraphs of plain text, no icons, no hierarchy beyond the 20 px title (`welcome.slint:68-122`).
16. **Vietnamese overflow risks** (labels are measured only for the popup footer):
   - Popup footer fits 369 of 398 px in Vietnamese by the test's estimate (see 7). Adding one more action breaks it.
   - History left pane is 180 px wide (`history.slint:309`); `FilterButton` text is elided at 13 px, so source-app and kind names are cut rather than wrapped.
   - Row meta, source app names and badges elide (`popup.slint:292-300`).
   - Settings capture-mode buttons: 12 + 15 + 25 characters ("Đang ghi nhớ", "Chế độ riêng tư", "Tạm dừng đến khi tiếp tục") in one row (`settings.slint:445-469`); fits at 780 px, not checked at 640 px.
   - Org dialog hints and dialogs wrap (`wrap: word-wrap`), card height is `min(content + 24, parent - 24)` (`organize.slint:194`), so very long hints can push the buttons out of view at 460 px height: not verified.
17. **Popup cannot be moved or resized** (frameless, fixed 420x560, `app.slint:19-23`), and the transform panel width is `min(600px, parent - 24)`, so in the popup it covers the whole window.
18. **Welcome Enter**: no default-button behaviour is declared (`welcome.slint:54-61`); not verified whether the stock `Button` handles Enter when focused.

## 6. Accessibility gaps

### 6.1 Contrast (WCAG 2.x, computed from the token hexes; `src/theme.rs:82-97` asserts only >= 4.5 for the listed pairs)

| Pair | Ratio | Result |
|---|---|---|
| text `#E7E9EC` on bg / surface / line | 13.72 / 12.32 / 9.11 | pass |
| text-muted `#A3A9B2` on bg / surface / line | 7.05 / 6.33 / 4.68 | pass |
| accent `#4C8DFF` on bg / surface / line | 5.21 / 4.68 / 3.46 | pass on bg and surface; fails on line |
| danger on bg / surface | 6.62 / 5.94 | pass |
| success / warning on bg | 9.58 / 9.95 | pass |
| bg on accent (chip "on", active transform item) | 5.21 | pass |
| accent text on its own 25 % tint over bg (active history filter, `history.slint:52,59`) | 3.57 | **fail** (13 px bold) |
| accent text on 25 % tint over surface (active settings tab `settings.slint:143,155`; chosen picker row `organize.slint:323,335`) | 3.24 | **fail** |
| accent hit highlight on marked row (`#273653`) | 3.77 | **fail** (13 px bold) |
| text-muted meta on marked + selected row (`#324A74`) | 3.75 | **fail** (11 px) |
| text-muted on marked row (no selection) | 5.10 | pass |
| disabled menu text (muted at 60 % on line / surface) | 2.68 / 3.23 | below 4.5; disabled controls are exempt |
| Non-text: selected row `surface` on `bg` | 1.11 | **fail** (3:1 for state indication) |
| Non-text: `line` on `bg` / on `surface` (borders, rules, inputs) | 1.51 / 1.35 | fail for input boundaries; decorative rules are exempt |
| Non-text: accent 1 px border on bg (search focus, list focus) | 5.21 | pass |

### 6.2 Roles, labels, focus order

| Gap | Where |
|---|---|
| Existing roles are good: chip = checkbox with checked state (`popup.slint:80-86`), row = list-item with selected/index/description (`:222-228`), list with item count (`app.slint:224-226`), `IconButton` label with icon role none (`popup.slint:164-177`), detail pane groupbox, settings tabs `tab` + `tab-list` (`settings.slint:144-147,306`), file rows, image alt | n/a |
| Footer `ActionButton`s are `role: button` with labels (`popup.slint:133-139`) but are not focusable: not reachable by keyboard or by Narrator's focus navigation unless it uses the accessibility tree (not verified) | `popup.slint:125-155` |
| Toast has role text only; no live-region concept is used, so a screen reader may not announce "Copied", "Clip deleted" or paste failures (not verified; Slint API support not checked) | `app.slint:337-344`, `history.slint:545-553` |
| Visible key hints are `accessible-role: none`; only line 1 text is also the search box description (`history.slint:284`), lines 2-3 are not exposed to screen readers | `history.slint:600,607,614` |
| Org strip and tags list have group role but tag rows are plain text with an unlabeled "x" button (accessible label is the glyph `x`/"×") | `organize.slint:140-160` |
| Context menu is a `PopupWindow`; entries are buttons but not focusable and have no `menu` role | `history.slint:620-737`; checklist note `platform-checklist.md:137` |
| `OrgDialog` pickers: `list` and `list-item` roles present, `choose` callback on action; selection moves through the host, not focus | `organize.slint:314-348` |
| Window-level: popup `panel` and modal overlays have `groupbox` role and label (`transform.slint:53-54,171-172`, `history.slint:742-743`), but nothing marks them modal | n/a |
| Focus order: popup relies on Slint default order (search, chips, list); the capture-phase `keys` scope sits outside it (`app.slint:102-104`) | `app.slint` |
| Settings: Esc closes overlays before the window; no focus is moved into an overlay when it opens and no focus return on close (not verified) | `settings_host.rs:1347-1363` |

### 6.3 Hit targets (logical px)

| Control | Size | Ref |
|---|---|---|
| Popup gear / close | 32 x 36 | `app.slint:170-171,180-181` |
| `ActionButton` | height 26, width label + 16 | `popup.slint:129-130` |
| `ChipView` | height 26 | `popup.slint:78` |
| Tag / chip remove "x" | 26 x 26 (24 px row) | `organize.slint:155,232` |
| Filter row / menu entry / tab | 28 / 30 / 36 | `history.slint:27,82`, `settings.slint:141` |
| Row | 68 | `popup.slint:67` |
| Detail pane buttons | 26 inside a 22 px row | `popup.slint:366` |

All interactive targets are >= 24 px (WCAG 2.2 AA 2.5.8). The smallest pair (26 px high chips and buttons at 4 px spacing) is dense for pointer use but compliant; none reaches the 32 px that touch-first guidance asks for.

## 7. Invariants to preserve

| Invariant | Evidence | What the redesign must not do |
|---|---|---|
| Masked clips: the UI lists them (UI caller gets `include_masked`, `crates/darkclip-app/src/api.rs:892`) but never loads content: `popup/detail.rs:45-47` (no request for masked), `:4-5`, pane shows only the note for kind 3 (`detail.rs:594-596`, `popup.slint:555-562`). Preview text of the masked row itself: not verified what text storage supplies | tests `detail.rs:707-798` | Show no text, image or file name for a masked row; keep the "Masked" badge and the note; do not add new surfaces (tooltips, hover cards, accessible labels, drag payload) that read clip content for masked rows; org actions stay disabled for masked (`OrgNothingToDo`, `i18n.rs:745`) |
| Source app names and previews are never logged (GC-06); `RowText` Debug hides the source (`row.rs:75-82`) | `row.rs`, `popup/chips.rs:3` | No logging or `Debug` of row texts when adding icon/kind fields |
| Paste-back safety: Enter and buttons only restore; only Ctrl+Enter (or opt-in "Enter pastes") pastes after the target checks; buttons, Alt+N and double-click never paste (D-056) | `popup/paste.rs:1-8`, `popup_host.rs:1114-1130` | Icon buttons keep `Via::Other`; a new "paste" button would need the same checks; tray-opened popup never pastes |
| OS and Darkshot exclusion markers are text-only in settings, no control (GC-07) | `settings.slint:6-7,501-540` | Keep the markers row non-interactive |
| i18n (GC-14): no user-visible literal in `.slint`; all text from `Strings`/`OrgStrings`/`SText`/`WText` or from Rust state | `ui/theme.slint:17`, both languages compile-time complete | Tooltips, icon labels, empty states and new section titles need `Msg` entries in `crates/darkclip-core/src/i18n.rs` in EN and VI. Known exception: the literal "\u{00d7}" button text (`organize.slint:153,231`) |
| GC-19: agents never screenshot, simulate input or touch the live clipboard or the default data dir | `docs/plans/global-constraints.md:19` | Verification is by tests, `--seed-demo-clips` with `--data-dir`, and owner manual rows |
| Task 414 glyph scan | `tests/no_font_glyphs.rs:117-140` scans `ui/*.slint`, `src/**/*.rs` and `i18n.rs` for U+2190..U+2BFF and >= U+1F000, literal or `\u{...}` escape, comments excluded; allow-list only `→` (`:13,19`); scanner self-test `:103-114` | Icons must be `Path` commands. No emoji, arrows other than U+2192, chevrons or stars written as text. `×` (U+00D7), `·` (U+00B7) and `...` are outside the range |
| Task 414 footer width | `tests/footer_budget.rs:15-18,38-50`: row width 420 - 2 - 20 = 398 px; 7 px per char, 16 px button padding, 4 px spacing; labels `UiCopy`, `UiFooterCopyPlain`, `UiFooterCopyAs`, `UiPin`/`UiUnpin`, `UiDelete`; asserts `<=` for EN and VI, pinned and not. Computed here: EN 327 px, VI 369 px | Any redesign that changes the popup width, padding, button padding/spacing, label set or label text must update this test in the same change; icon-only buttons make it moot but the test's constants then become stale. `memory-bank/systemPatterns.md:29` cites both tests |
| Software renderer (D-055): 6.9 MiB private bytes hidden, 11.7 MiB after 20 shows, 0 idle CPU, against 52 / 72 MiB and ~0.2 % for the GPU renderer; no font fallback (symbols draw nothing); selection in `src/renderer.rs:1-12` | `docs/testing/perf-windows.md:150` | No features that need GPU (shaders, blur, `drop-shadow-*`, large animated layers). Path icons are cheap; prefer rectangles and 1-bit alpha tints. Image/SVG assets: not verified to cost memory; Path commands are the proven option |
| Popup open time NFR-01: p95 <= 100 ms, measured 21.1 ms with 10,000 clips (`docs/product/mvp-scope.md:59`, `docs/testing/perf-windows.md:41-49,169`); window created hidden at start, shown near the cursor | `popup/place.rs`, `popup_host.rs` | Keep the component tree light (rows are a `ListView`, 68 px fixed; `shown-until` pages). Do not add per-row `if` subtrees with layouts beyond what exists. `scripts/measure-popup.ps1` re-measures |
| Memory NFR-05 and idle CPU NFR-04 met on Windows only with the software renderer | `renderer.rs:3-6` | Adding timers or animations that run while hidden breaks idle CPU; animations must stop when the window is hidden |
| Always dark (D-037): `Palette.color-scheme = dark` in each window's `init` (`app.slint:97-99`, `history.slint:243-245`, `settings.slint:284-286`, `welcome.slint:50-52`) so stock widgets stay dark | n/a | Keep when moving to a custom control set; stock widgets remain in settings/welcome until replaced |
| Colour values have a single source in `src/theme.rs` with a WCAG >= 4.5 test over 9 pairs (`theme.rs:82-97`) | `theme.rs` | New tokens (hover, selected, scrim, on-accent) need their own pairs in that test; the failing pairs of 6.1 are not covered by it today |
| Search debounce 60 ms, loading hint after 150 ms (`popup_host.rs:52,54`); keys are decided in Rust (`key` callback returns `bool`) | `popup_host.rs`, `popup/keys.rs`, `history/keys.rs` | The key maps are pure and unit tested; the redesign changes markup, not these maps, unless an action is added (then add a test) |
| Platform claims: only Windows is verified; macOS/Linux UI claims need `platform-compatibility.md` marked T | `CLAUDE.md` rules | Do not state cross-platform support for the new UI |
| No network, no telemetry; no new crates without justification (GC-08, GC-12) | `docs/plans/global-constraints.md` | SVG files rendered via Slint `@image-url` would add asset loading; Path icons add no dependency |

## 8. Reusable pieces

Keep as is:

| Piece | Why |
|---|---|
| `Theme` global with `src/theme.rs` as the source of truth, plus the contrast test | Extend with tokens (radius, spacing, size scale, alpha tints, scrim, on-accent) rather than replace |
| `Strings`/`OrgStrings`/`SText`/`WText` + `set_shared_strings` and the i18n completeness guarantee | Add keys for tooltips and labels in the same table |
| `Icon` + `IconKind` (`Path` approach, 24x24 viewbox) | Proven under the software renderer and the glyph scan; extend the enum and the `commands:` ternary or split per-icon components |
| `IconButton` (role button, label, icon role none) | Add focus state, tooltip, size variants; label contract stays |
| `ChipView` and `FilterButton` accessible pattern (checkbox + checked + default action, key handling + focus reporting to Rust via `focus-report`) | The Rust `ChipFocus` logic (`popup/keys.rs:92-119`) depends on these callbacks |
| `RowView` structure, `RowMetrics.height`, `ClipRow`/`Seg` models, pin bar, `accessible-*` on rows, double-click and pointer-event contract for history (`pressed`, `context`) | Hosts, scrolling math (`scroll-to`, `page-rows`, `shown-until`) and tests depend on a fixed row height |
| `DetailState` + `DetailPane` kinds 0..3 and the Rust `DetailModel` (`popup/detail.rs`) | Privacy rules live in the model; markup is pure layout |
| `TransformPanel`, `EditPanel`, `OrgDialog` state structs (`OrgDialogState`) and `DialogKey` routing (`organize/dialog.rs`) | Modal behaviour and key swallowing are tested in Rust |
| Pure key maps and their tests (`popup/keys.rs`, `history/keys.rs`, `popup/transform.rs`) and the `key(...) -> bool` capture-phase pattern (`app.slint:102-110`, `history.slint:248-256`, plus the FocusScope wrappers for dialogs with a focused text box, `history.slint:789-856`) | Changing markup does not change them; new actions need a test |
| Rust view-model builders `clip_row`, `row_text`, `split_two_lines`, `popup_chips`, `history_filters`, `set_detail_if_changed` (`common.rs:27-125`, `row.rs`, `chips.rs`) | Add an icon-kind field instead of rewriting; `kind_icon` (`row.rs:29-38`) becomes an enum index |
| Window placement, DPI scaling and remembered sizes (`popup/place.rs`, `history/place.rs`, `winsys.rs`) | If the popup size changes, change `POPUP_LOGICAL` (`place.rs:5`) and `app.slint:22-23` together |
| Overlay scrim + `TouchArea {}` to swallow clicks (4 copies) | Candidate for one shared `Modal`/`Scrim` component |
| The `Palette.color-scheme = dark` init line | Needed while any std-widget remains |

Candidates to build once and share: `Button` (primary / secondary / ghost / danger, with focus ring and optional icon), `IconButton` with tooltip, `Tooltip` (none exists; a `PopupWindow` or an overlay `Rectangle` driven by `has-hover` after a delay), `Modal` card, `Segmented`/`Toggle` for the capture-mode and checkbox groups, `Field` (label, input, error), `SectionHeader`, `KeyHint` chip (keys are known in the Rust maps), `Badge` (exists, private to `popup.slint`), kind icons for `ClipType` (7 kinds, `popup/row.rs:15-39`).
