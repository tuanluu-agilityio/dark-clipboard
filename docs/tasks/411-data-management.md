# Task 411: Data management (storage usage, cleanup)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/411-data-management` · **Review:** `security-reviewer` (opus; deletion)
**Global Constraints:** GC-06, GC-10, GC-11, GC-14, GC-18. **Plan:** Phase 5 Task 411; owner prompt phase3 §7 (D-057).

## Requirements
1. Settings > History shows storage usage: DB size, blob bytes, clip counts by kind, pinned and favorite counts (content-free stats).
2. Cleanup tools, each with a confirmation that shows the count: clear history keeping pinned **and favorites** (extend the existing clear); delete clips older than N days (non-pinned, non-favorite); remove orphaned blob files now (the startup sweep on demand, reporting files/bytes freed); compact database (`VACUUM` on the writer, refused when free disk space is below the DB size).
3. All through `App` / actor commands; deletion is real deletion (data-model §6).

## Tests (first, failing)
Clear keeps pinned and favorites; delete older-than respects pin/favorite; orphan sweep reports counts and never deletes a referenced blob; vacuum keeps data and search results identical; view-model confirmation counts.
