# UI redesign: design system

Status: **approved** (D-078, 2026-11-24). Part of the UI redesign package (`README.md` in this folder).
Applies to every Slint window in `apps/desktop/ui/`. Dark only (D-037).

## 1. Principles

1. **Icons first, words where they carry meaning.** Frequent actions are icon buttons with a tooltip
   (name + shortcut) and a full `accessible-label`. Words stay for content, section titles, settings
   explanations and destructive confirmations.
2. **Quiet chrome, loud content.** Clip text is the brightest thing on screen; chrome uses `text-2`
   and `border`. One accent colour, used only for selection, focus and the primary action.
3. **Keyboard-first.** Every action has a key; every focused element shows a visible focus ring;
   the mouse is optional.
4. **Compact, not cramped.** 4 px grid, 13 px body text, rows sized for two lines.
5. **Cheap to render.** Flat fills and 1 px borders. No blur, no gradients, no shadows, no
   animations longer than 120 ms (only colour fades on hover/selection; none when reduced motion is
   asked for, and none required for function).

## 2. Colour tokens

Set from `apps/desktop/src/theme.rs` (still the single source of truth) into the `Theme` global.
Contrast ratios are WCAG 2.x, computed (script output recorded in `05-acceptance.md`).

| Token | Value | Use | Contrast (on canvas / surface) |
|---|---|---|---|
| `canvas` | `#111318` | Window background | — |
| `surface` | `#1B2028` | Panels, sidebar, inputs, cards | — |
| `raised` | `#232935` | Hover row, pressed button, menu background | — |
| `selected` | `#262C45` | Selected row (with accent bar) | — |
| `border` | `#2D3440` | Dividers, card outlines (decorative) | 1.48 / 1.31 (decorative only) |
| `border-strong` | `#4A5263` | Input outline, switch track off (component boundary) | 2.37 on canvas, used with a surface fill |
| `text` | `#F3F5F9` | Content, titles | 17.0 / 15.0 |
| `text-2` | `#A1AAB8` | Labels, descriptions, inactive icons | 7.9 / 7.0 |
| `text-3` | `#8F98A8` | Metadata (time, source app, counts) | 6.4 / 5.6 (≥ 4.7 on `selected`) |
| `accent` | `#7887FF` | Selection bar, focus, primary button, active filter | 5.9 / 5.2 |
| `on-accent` | `#111318` | Text/icon on accent fills | 5.9 |
| `focus` | `#9AA6FF` | 2 px focus ring | 8.2 / 7.2 |
| `success` | `#5FD4AA` | "Copied", capture on | 10.2 / 9.0 |
| `warning` | `#F2C14E` | Capture paused, shortcut warning | 11.1 / 9.8 |
| `danger` | `#FF7A85` | Delete, errors, sensitive badge | 7.4 / 6.5 |

Rules: text never uses `border`/`border-strong`; `text-3` is never used on `raised`; meaning never
relies on colour alone (badges carry an icon and an accessible label).

## 3. Typography

System UI font (Segoe UI on Windows, as today); mono font unchanged (`Theme.mono-font`).

| Token | Size / weight | Use |
|---|---|---|
| `title` | 15 px / 600 | Window and section titles |
| `body` | 13 px / 400 | Clip previews, setting labels, menu items |
| `body-strong` | 13 px / 600 | Group headers in lists, button labels |
| `small` | 12 px / 400 | Descriptions, chips, tooltips |
| `meta` | 11 px / 400 | Row metadata, counts, key caps |
| `overline` | 11 px / 600, letter-spacing 0.4 px | Section labels ("PINNED", "RECENT") |

Vietnamese diacritics need line height ≥ 1.35× font size: rows reserve 18 px per 13 px line.

## 4. Spacing, size, radius

- Spacing scale: 2, 4, 8, 12, 16, 24 px. Panel padding 12 px; list horizontal padding 8 px.
- Radius: 4 px (key caps, badges), 6 px (buttons, inputs, chips, rows), 10 px (popup window, cards,
  menus).
- Control heights: 28 px (icon buttons, chips, compact inputs), 32 px (search field, text
  buttons), 36 px (settings rows minimum).
- Hit targets ≥ 28 × 28 px (WCAG 2.5.8 minimum is 24).
- Row heights: popup 48 px (one preview line + meta line); history 56 px (two preview lines + meta).

## 5. Icons

- Set: **Lucide** (ISC licence, GPLv3-compatible), stroke 2 px on a 24-unit grid, drawn at 16 px
  (18 px in the sidebar/nav). Files are copied into `apps/desktop/ui/icons/*.svg` (only the icons
  used, about 42) and embedded at compile time with `@image-url`. Tint with `colorize:` from `Theme`.
  Licence text added to `about.hbs`/third-party notices. No new crate.
- Verified 2026-11-24 (throwaway check, not in the repo): Slint 1.19 software renderer draws a
  stroked SVG with `colorize` correctly (284 tinted pixels, 0 off-colour pixels on a 48 × 48 buffer).
  The `Path`-based `Icon` of Task 414 stays as a fallback.
- Default tint `text-2`; hover `text`; active `accent`; destructive hover `danger`.
- The Task 414 glyph-scan test stays: icons are never text glyphs.

Icon map (Lucide names): search, settings, x, pin, pin-off, star, lock, shield-check, eye-off
(masked), trash-2, copy, clipboard-paste (paste), type (plain text), wand-sparkles (transform),
pencil (edit copy), sticky-note (note), tag, folder (collection), folder-plus, layers (all),
file-text (text), link, code, image, file (files), clock (recent), history (history window),
more-horizontal, check, undo-2, keyboard, pause, play, database, hard-drive-download (backup),
upload / download (import/export), info, refresh-cw (update check), panel-right (toggle preview),
list-checks (select mode), chevron-right, chevron-down.

## 5b. App icon (owner chose option A, D-078)

One icon for the exe, taskbar/Alt+Tab, window title bars and tray (`mockups/app-icon.html`, symbol `A`).
Tile shared with Darkshot: 56-unit view box, `rect x=2 y=2 w=52 h=52 rx=13`, fill `#202328`, 1.2-unit
border `#3A3E45`. Mark: clipboard board `rect x=16 y=15 w=24 h=30 rx=4.5`, stroke `#7887FF` width 4,
no fill; clip `rect x=21.5 y=10.5 w=13 h=8 rx=2.5` fill `#F3F5F9`; two lines `M22 28h12 M22 35h8`
stroke `#F3F5F9` width 3.5, round caps. Sizes 16, 20, 24, 32, 40, 48, 64, 256; 16/20/24 hand-tuned
on the pixel grid. Tray states keep the icon and add a bottom-right badge (circle r 12 at 43,43 with a
3-unit `#111318` ring): paused/off amber `#F2C14E` with pause bars, private `#A1AAB8` with a struck
eye, unavailable `#FF7A85` with "!".

## 6. Components (new shared library `apps/desktop/ui/kit/`)

| Component | Replaces | Notes |
|---|---|---|
| `Icon` | glyph text, Task 414 `Path` icons | `name` enum → `@image-url`; `size`, `tint` |
| `IconButton` | `ActionButton` text buttons | 28 × 28, icon 16, tooltip, `accessible-label`, focus ring, optional `danger`/`active` |
| `Button` | text buttons in dialogs/settings | `primary` (accent fill) / `secondary` (surface + border-strong) / `danger` |
| `Tooltip` | key-hint footers | appears after 500 ms hover or on focus; text = name + key cap; one at a time |
| `KeyCap` | key hints in text | 11 px on `raised`, radius 4 |
| `SearchField` | search `LineEdit` | search icon, clear (x) button, 32 px |
| `FilterTab` | `ChipView` | icon + optional label; segmented group; `checkbox` role kept |
| `ClipRow` | popup/history rows | kind icon, preview, meta, badge icons, hover/selected actions |
| `SectionHeader` | — | overline label + count, collapsible in history sidebar |
| `NavItem` | `SectionTab`, history filter buttons | icon + label + count; collapses to icon in a rail |
| `Card`, `SettingRow` | settings groups | label + description left, control right |
| `Switch` | checkboxes in settings | 32 × 18, `checkbox` role, label tied |
| `Menu` / `ContextMenu` | — (new) | keyboard navigable; Slint `ContextMenuArea` if it renders under the software renderer (check in U0), else a `PopupWindow` menu |
| `Toast` | toast/undo row | floats just above the status line, full list width minus 12 px margins; icon + short text + Undo; 5 s (4 s errors) |
| `EmptyState` | empty/no-result texts | icon 32 + one line + one hint |

States every interactive component implements: default, hover, pressed, focused (2 px `focus`
ring, offset 1 px), selected/active, disabled (`text-3`, no hover).
