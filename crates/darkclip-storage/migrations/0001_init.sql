-- Schema v1 (docs/architecture/data-model.md §4, normative).
-- `PRAGMA journal_mode = WAL` and `PRAGMA foreign_keys = ON` are connection settings
-- that SQLite ignores inside a transaction, so db.rs applies them on every open,
-- before this file runs. Everything below runs in one transaction (migrate.rs).

CREATE TABLE clips (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  preview TEXT NOT NULL,
  content_hash BLOB NOT NULL UNIQUE,          -- 32 bytes
  size_bytes INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  last_copied_at INTEGER NOT NULL,
  last_used_at INTEGER,
  copy_count INTEGER NOT NULL DEFAULT 1,
  use_count INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  pin_order INTEGER,
  source_app_id TEXT, source_app_name TEXT, source_platform TEXT, source_confidence TEXT,
  sensitivity_level TEXT NOT NULL DEFAULT 'none',
  sensitivity_categories TEXT,                -- JSON array of category names
  note TEXT,
  expires_at INTEGER,
  meta TEXT NOT NULL DEFAULT '{}'             -- ClipMetadata JSON
);
CREATE INDEX clips_recent ON clips(pinned DESC, last_copied_at DESC);
CREATE INDEX clips_expiry ON clips(expires_at) WHERE expires_at IS NOT NULL;

CREATE TABLE representations (
  clip_id TEXT NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  mime TEXT NOT NULL,
  platform_format TEXT,
  size_bytes INTEGER NOT NULL,
  inline_data BLOB,                            -- NULL when stored as blob
  blob_sha256 BLOB,                            -- NULL when inline
  width INTEGER, height INTEGER,
  PRIMARY KEY (clip_id, ord)
);
CREATE INDEX reps_blob ON representations(blob_sha256) WHERE blob_sha256 IS NOT NULL;

CREATE VIRTUAL TABLE clips_fts USING fts5(
  preview, body, note, tags,
  content='', contentless_delete=1,            -- SQLite >= 3.43
  tokenize = 'unicode61 remove_diacritics 2'
);
-- rowid mapping: clips_fts.rowid = clip_rowid.rowid
CREATE TABLE clip_rowid (rowid INTEGER PRIMARY KEY, clip_id TEXT UNIQUE NOT NULL REFERENCES clips(id) ON DELETE CASCADE);

PRAGMA user_version = 1;
