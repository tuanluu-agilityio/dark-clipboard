# Task 403: Organization UI (collections, tags, notes, favorites)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/403-organization-ui` · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-14, GC-18, GC-19. **Plan:** Phase 5 Task 403; owner prompt phase3 §1–2 (D-057); journey J7.

## Requirements
1. **History window** left pane: Favorites, user collections (create, rename, delete with confirmation; clips are kept), tags with counts; selecting one filters the list. Drag is optional; every action has a keyboard path (J7).
2. **Detail pane:** favorite toggle, note editor (≤ 4,000 chars, counter, Save/Cancel), tag editor (chips, add/remove; an invalid tag shows the i18n message).
3. **Multi-select:** add to collection, remove from collection, add tag, favorite/unfavorite for the marked rows (confirmation shows the count).
4. **Popup:** favorite star on rows; filter chips for Favorites and collections (keyboard reachable); a key to toggle favorite on the selected row (document it in the popup help and README).
5. "Snippets" (D-037): documented as a user collection plus pinned clips; no template engine. README + CHANGELOG.
6. All strings en + vi; no business logic in the UI (view-models call `App`). New windows/dialogs are cleared on close like D-052.

## Tests
View-model tests: collection create/rename validation, delete confirmation, multi-select counts, note length limit, tag chip validation, popup favorite toggle mapping. Owner checklist rows for each flow (keyboard only).
