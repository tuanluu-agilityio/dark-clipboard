# UI redesign: wireframes

Status: **approved** (D-078, 2026-11-24). Visual mockups: `mockups/index.html` (open in a browser; static HTML, not the app).
Sizes are logical pixels at 100 % scaling. `[i]` = icon button (28 × 28), `(i)` = icon only (no button).

## 1. Quick Popup (440 × 520; 760 × 520 with preview)

```
┌──────────────────────────────────────────────────────┐
│ [(search) Search clipboard            Ctrl K] [▯][⟲][⚙][x] │  header 48: search 32 + preview, history, settings, close
│ ┌All│(txt)(link)(code)(img)(file)│(pin)(star)┐ [⌄]   │  filter segment 30 + "more filters" menu
│ PINNED 2                                             │  overline 22
│▌(kind) Weekly sync: Tuesdays 10:00, room…  [⧉][pin][…]│  selected row 48: accent bar + row actions
│ (kind) cargo test -p darkclip-storage …      (pin) 2 │  other rows: badges + Alt number
│ RECENT 128                                           │
│ (kind) https://github.com/slint-ui/…        (star) 3 │
│ (kind) Hidden: looks like a password        (lock) 5 │  masked: italic text-3, lock badge, no preview
│ …                                                    │
│ ┌ (trash) Clip deleted          (undo) Undo Ctrl Z ┐ │  toast floats just above the status line (does not replace actions)
│ ● Capturing              [Enter] Copy [Ctrl Enter] Paste [F1] Keys │  status 30
└──────────────────────────────────────────────────────┘
```

- Rows are 48 px: 1 preview line (13 px) + 1 meta line (11 px, source · age · extra). Kind is an icon
  (no letter codes, no kind word in meta). The Alt+N number shows for rows 1–9 only.
- Row actions (copy, pin, more) appear only on the selected or hovered row; "more" opens the row menu
  (Copy, Copy as plain text, Copy as…, Pin, Favorite, Delete), the same items as keys. Paste stays
  keyboard-only (Ctrl+Enter, or Enter with "Enter pastes"), as today: no new paste path.
- "More filters" (chevron) opens a menu: Today, Last 7 days, Collections ›, Apps ›. When one is on,
  it shows as a removable chip right of the segment (only active extras take space).
- Preview pane (Space, →, or [▯]): the window widens to 760 px; the list keeps its height. Pane:
  header (kind icon + title + plain-text/transform icons), content (text, image, file list, masked
  note), facts (source, time, size). Placement keeps the window on the monitor (flip left).
- Empty and error states: `EmptyState` (icon 32 + one line + one hint) centred in the list area.
- Copy as… keeps its modal panel (restyled: `Menu`-like list on the left, preview right).

## 2. History manager (preferred 1060 × 660; minimum 640 × 460)

Wide (≥ 1000 px):

```
┌ sidebar 216 ─┬ list (flex, min 320) ──────────────────────┬ detail 340 ─────────────┐
│ (all) All 1284│ All clips [(search) Search 1,284  Ctrl F] [Any time ⌄][☑]│ [⧉][T][✨][✎]   [pin][★][🗑] │
│ (pin) Pinned  │──────────────────────────────────────────│ ┌ preview box ────────┐ │
│ (star) Favs   │▌(kind) SELECT id, kind, created_at…  (star)│ │                     │ │
│ TYPES         │        WHERE pinned = 1 …                 │ └─────────────────────┘ │
│ (txt) Text    │        DBeaver · 1 h · (tag) sql          │ Type / From / Copied    │
│ …             │ (kind) https://github.com/…               │ TAGS  [sql x][work x][+]│
│ COLLECTIONS [+]│ …                                         │ NOTE  [ text … ]        │
│ (folder) Snip.│                                           │ COLLECTIONS [Snippets][+]│
│ TAGS          │                                           │                         │
│ APPS ›        │ 1,284 clips · 38 MB      [Menu] Actions [F1] Keys │                  │
│ (⚙) Settings  │                                           │                         │
└───────────────┴───────────────────────────────────────────┴─────────────────────────┘
```

- Rows 56 px: preview line 1 (13 px), preview line 2 (12 px, `text-2`), meta (11 px: source · age ·
  first tag or collection). No `Alt+N` prefix in history (audit §5.9).
- Sidebar groups: Library (All, Pinned, Favorites), Types, Collections (+ to create; right-click or
  Menu key on one → rename/delete), Tags, Apps (collapsed by default). Each item has a count.
  These are the existing `FilterItem`s regrouped; one item active at a time per group as today.
- Detail panel toolbar: Copy, Copy plain, Copy as…, Edit a copy… | Pin, Favorite, Delete. Tags,
  note and collections are inline sections that open the existing org dialogs (restyled).
- Bulk: with ≥ 2 marked rows, a bulk bar (accent outline) appears above the list: "N selected" +
  Pin, Favorite, Add to collection, Tag, Export, Delete, Clear. Checkboxes appear in rows while
  select mode is on (the [☑] toggle or as soon as a second row is marked).
- Context menu: the row menu from §1 plus Edit a copy…, Add to collection ›, Remove from collection,
  Tags…, Note…; keyboard: Menu key / Shift+F10, arrows, Enter, Esc; → opens a submenu.

Narrow:

| Width | Sidebar | Detail |
|---|---|---|
| ≥ 1000 | 216 px | 340 px panel |
| 820–999 | 52 px icon rail (tooltips; Ctrl+B expands as an overlay) | 300 px panel |
| 640–819 | 52 px rail | drawer over the list from the right (→ / Enter on preview opens, Esc / ← closes) |

## 3. Settings (preferred 860 × 640; minimum 680 × 480)

```
┌ nav 220 ─────────────┬ content (scrolls) ───────────────────────────────┐
│ (sliders) General    │ Title 18 px                                       │
│▌(shield) Capture & P.│ one-line description (text-2)                     │
│ (db) History & Stor. │ ┌ CARD TITLE ────────────────────────────────────┐ │
│ (⇄) Data & Transfer  │ │ Label                                 [control] │ │
│ (i) About            │ │ description (text-3)                            │ │
│                      │ ├─────────────────────────────────────────────────┤ │
│                      │ │ …                                               │ │
│                      │ └─────────────────────────────────────────────────┘ │
│                      ├───────────────────────────────────────────────────┤
│                      │ ● 1 unsaved change          [Revert] [✓ Save Ctrl S] │  only when dirty
└──────────────────────┴───────────────────────────────────────────────────┘
```

Section contents (every existing setting keeps its place in exactly one section):

| Section | Cards (existing controls, regrouped) |
|---|---|
| General | Shortcut (field + warning), Language, Start at login (+ notes), Popup position, Close on focus loss, Enter pastes (with its explanation), Show welcome again |
| Capture & Privacy | Capture mode (3 cards: Recording / Private / Paused), **Always protected** (OS and password-manager markers, Darkshot marker: lock icon, no control, GC-07), Sensitive content (existing options), Don't save from these apps (list, built-in rows labelled, Add app… / Pick running app…), Types to save (switch per kind), skip summary text |
| History & Storage | Limits (max items, max age, max storage), usage stats, Clean up (older than N days, keep pinned), Clear history (danger) |
| Data & Transfer | Backup, Restore, Export, Import, Data folder (path, size, Open) |
| About | Version, update check (opt-in switch, Check now, release link), capabilities, licences, data folder link |

- Controls: `Switch` for booleans, `Select` (restyled ComboBox) for choices, `Field` for numbers and
  text with inline error under the row, `Button` secondary for actions, danger only for clear/delete.
- The five confirmation overlays become one `ConfirmDialog` component (title, body, danger/primary
  button), same callbacks.
- Welcome window: same card style (icon + title + two short paragraphs + switch + primary Start),
  restyled in the Settings task; behaviour unchanged (capture off until Start).
