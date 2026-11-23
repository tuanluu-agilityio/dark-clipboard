-- Schema v2: collections, tags, notes, favorites (Task 401; data-model §3, §7).
-- Runs in one transaction (migrate.rs) after db.rs has backed the database up to
-- `history.db.pre-v2.bak`; any failure rolls everything back, version 1 included.
-- `PRAGMA foreign_keys = ON` is a connection setting applied by db.rs on every open.

CREATE TABLE collections (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  kind TEXT NOT NULL CHECK (kind IN ('user', 'system')),
  system_key TEXT UNIQUE CHECK (system_key IS NULL OR system_key = 'favorites'),
  color TEXT,
  sort TEXT NOT NULL DEFAULT 'recent' CHECK (sort IN ('manual', 'recent')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK ((kind = 'system') = (system_key IS NOT NULL))
);

CREATE TABLE collection_clips (
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  clip_id TEXT NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
  ord INTEGER NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (collection_id, clip_id)
);
-- The clip side of the cascade, the favorites check in retention, and get_clip.
CREATE INDEX collection_clips_clip ON collection_clips(clip_id);

CREATE TABLE tags (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
);

CREATE TABLE clip_tags (
  clip_id TEXT NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (clip_id, tag_id)
);
-- The tag side of the cascade and the unused-tag cleanup.
CREATE INDEX clip_tags_tag ON clip_tags(tag_id);

-- The system Favorites collection. Fixed id (darkclip_core::FAVORITES_COLLECTION_ID);
-- code finds it by system_key. Storage refuses to rename or delete it.
INSERT INTO collections (id, name, kind, system_key, sort, created_at, updated_at)
VALUES ('00000000-0000-7000-8000-000000000001', 'Favorites', 'system', 'favorites', 'recent',
        CAST(unixepoch('subsec') * 1000 AS INTEGER), CAST(unixepoch('subsec') * 1000 AS INTEGER));

-- Belt and braces under the storage checks: the system row cannot be removed or renamed
-- by any statement, including a cascade from a future migration.
CREATE TRIGGER collections_system_no_delete BEFORE DELETE ON collections
WHEN OLD.kind = 'system' BEGIN SELECT RAISE(ABORT, 'system collection'); END;
CREATE TRIGGER collections_system_no_update BEFORE UPDATE OF name, kind, system_key ON collections
WHEN OLD.kind = 'system' BEGIN SELECT RAISE(ABORT, 'system collection'); END;

PRAGMA user_version = 2;
