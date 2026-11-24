# UI redesign: interaction and keyboard specification

Status: **proposed**. Baseline: the current key maps (audit `01-audit.md` §3). **Every existing key
keeps its meaning.** Rows marked **new** are additions; none removes or rebinds a key.

## 1. Rules that apply everywhere

1. Every action reachable by mouse is reachable by keyboard, and the reverse (closes audit §5.6).
2. Every interactive component is a `FocusScope` with the 2 px `focus` ring; Tab order follows reading
   order (left to right, top to bottom); Shift+Tab reverses.
3. Tooltips show on hover (500 ms) **and on keyboard focus**: name + key cap. Tooltip text comes from
   i18n (new keys, EN + VI). The `accessible-label` is the full action name; `accessible-description`
   carries the shortcut.
4. **Esc unwinds one layer at a time**: open menu → tooltip/drawer/preview → dialog → marks →
   close window. Never more than one layer per press.
5. Key handling stays in Rust pure key maps (`src/popup/keys.rs`, `src/history/keys.rs`) with unit
   tests; Slint only forwards keys (existing capture-phase `key(...)` callback).
6. Clipboard behaviour is unchanged: Enter, buttons and double-click copy; only Ctrl+Enter (or Enter
   when "Enter pastes" is on) pastes; paste-back checks (T-19) unchanged.
7. Menus: Up/Down move, Home/End first/last, Enter or Space activates, → opens a submenu, ← or Esc
   closes it, typing a letter jumps to the next item starting with it. Focus returns to the row.
8. A disabled action is shown disabled (not hidden) in menus, hidden in row hover actions.

## 2. Quick Popup

Opening: the global shortcut (unchanged) opens the popup at the cursor with focus in the search box,
the query empty and the first row selected (unchanged).

| Key | Action | Status |
|---|---|---|
| Typing | filters the list (search keeps focus) | existing |
| Up / Down, PgUp / PgDn | move selection | existing |
| Home / End (list owns keys), Ctrl+Home / End | first / last | existing |
| Enter / Shift+Enter / Ctrl+Enter | copy / copy plain / paste | existing |
| Alt+1..9 | copy n-th visible row (number shown at row end) | existing |
| Space, → / ← (list owns keys) | toggle / open / close preview pane | existing (pane moves to the side) |
| Del, Ctrl+Z | delete with undo; undo | existing |
| Ctrl+P / Ctrl+D / Ctrl+T | pin / favorite / Copy as… | existing |
| Ctrl+H | open history | existing |
| Esc | close layer, then close popup | existing (layering rule §1.4) |
| Ctrl+K | focus search and select its text | **new** |
| Ctrl+, | open Settings | **new** (today gear only) |
| Shift+F10, Menu key | open the row menu at the selected row | **new** (popup) |
| F1 (also Ctrl+/) | show the shortcuts sheet (overlay listing this table, from i18n) | **new**; replaces hidden key hints |
| Ctrl+1..9 | select filter segment n (1 = All) | **new** |
| Ctrl+Shift+F | open "More filters" menu | **new** |
| Tab / Shift+Tab | search → filter segment → more-filters → list → header buttons | changed order: buttons now focusable |

Filter segment: one Tab stop; ←/→ move between segments, Enter/Space toggles (radio for type,
toggle for Pinned/Favorites: same semantics as today's chips, which are kept in Rust `chips.rs`).

Mouse: click selects, double-click copies (existing); hover shows row actions; right-click opens the
row menu (**new** in popup); wheel scrolls the list; the filter segment never scrolls sideways (the
overflow chips move into "More filters").

Toast: floats above the status line for 5 s (4 s errors), does not hide any control; Undo is a
button inside it and Ctrl+Z works while it shows (existing timing). Screen readers get the toast as
a live status (same text).

## 3. History manager

| Key | Action | Status |
|---|---|---|
| All existing keys of `history/keys.rs` (navigation, Shift/Ctrl ranges, Space mark, Enter, Shift+Enter, Del, Menu key, Ctrl+P/T/E/D/N/G, Ctrl+L, Ctrl+Shift+L, Ctrl+Shift+N/R/Del/E, Ctrl+A, Esc) | unchanged | existing |
| Ctrl+F, Ctrl+K | focus search | **new** |
| Shift+F10 | context menu (same as Menu key) | **new** |
| Ctrl+B | expand/collapse sidebar (rail ↔ full) | **new** |
| F6 / Shift+F6 | cycle regions: sidebar → list → detail | **new** |
| → / Enter on a drawer-width window | open detail drawer; Esc / ← closes | **new** (narrow only) |
| Ctrl+, | Settings | **new** |
| F1, Ctrl+/ | shortcuts sheet (replaces the 3 hint lines) | **new** |

Sidebar: one Tab stop; Up/Down move, Enter/Space activates, Menu key on a collection opens
rename/delete, group headers collapse with ←/→. Detail panel: icon toolbar is one Tab stop with
←/→ between buttons (toolbar pattern), then tags, note, collections in order.

Bulk: marking 2+ rows shows the bulk bar; its buttons act on all marked rows (existing bulk
callbacks); Esc clears marks (existing).

## 4. Settings

| Key | Action | Status |
|---|---|---|
| Ctrl+1..5 | go to section n | **new** |
| Ctrl+S | Save | **new** |
| Esc | close overlay, else hide window (unsaved changes stay pending as today) | existing |
| Tab / Shift+Tab | nav → section controls in order → save bar | changed: nav items focusable |
| Up/Down in nav | move section | **new** |
| Enter on a dialog | primary action; Esc cancels | existing behaviour, one `ConfirmDialog` |

Switches toggle with Space; selects open with Alt+Down or Space and choose with arrows + Enter.

## 5. Accessibility semantics (Slint accessible properties)

| Component | Role | Label / description |
|---|---|---|
| `IconButton` | `button` | action name / shortcut |
| Filter segment item | `checkbox` (as today) | kind name, checked state |
| `NavItem` | `tab` in a `tab-list` (settings), `list-item` with checked (history sidebar) | name + count |
| `ClipRow` | `list-item` | existing `label` / `description` from Rust (kind, preview, age, source, badges) |
| Badges (pin, star, lock, masked) | decoration; included in the row description text | — |
| `Switch` | `checkbox` | setting label / description |
| Menu item | `button` (Slint has no `menu-item` role in 1.19: check in Task 700) | name / shortcut |
| Toast | `text` with live update (if supported; otherwise focus is not moved) | toast text |
