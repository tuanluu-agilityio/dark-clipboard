-- Schema v3: the no-export flag (Task 405 security review; privacy-design §3).
-- A clip captured with the Windows `CanUploadToCloudClipboard` = 0 marker (`PrivacyMarker::NoCloud`)
-- must never be exported or synced. Clips stored before this migration carry no flag.
-- Runs in one transaction after db.rs has backed the database up to `history.db.pre-v3.bak`.

ALTER TABLE clips ADD COLUMN no_export INTEGER NOT NULL DEFAULT 0 CHECK (no_export IN (0, 1));

PRAGMA user_version = 3;
