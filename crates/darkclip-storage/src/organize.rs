//! Collections, favorites, tags and notes (schema v2, Task 401; data-model §3).
//!
//! Rules:
//! - Every mutation is one `BEGIN IMMEDIATE` transaction on the writer; reads work on a
//!   read-only store too.
//! - Favorites is the system collection with `system_key = 'favorites'` (created by
//!   migration 2). It can never be renamed or deleted (`InvalidArgument`); the schema
//!   refuses it as well (triggers in `0002_organization.sql`).
//! - Membership is idempotent. `ord` stays dense (0..n) after a removal or a reorder; a
//!   clip deleted from the history leaves a gap that the next reorder closes. New members
//!   go to the end.
//! - Tag names are normalised by `darkclip_core::Tag::validate_name` (trim, lowercase,
//!   NFC, `[\p{L}\p{N}_-]+`, at most 40 characters); a clip carries at most 20. Tags no
//!   clip uses are deleted in the same transaction (here and on clip deletion).
//! - Notes are at most 4,000 characters; an empty note clears it.
//! - Masked clips: a note or tags are refused (`InvalidArgument`); clearing is allowed.
//!   The FTS row of a masked clip holds only its masked preview.
//! - The FTS table is contentless: a change of note or tags deletes the row and inserts it
//!   again with every column, the body re-read from the text representation (inline or
//!   blob) under the same rules as capture (first 32 KiB).
//! - Errors never carry a name, note or any other user text (GC-06).

use crate::blobs;
use crate::clips::{decode_err, enum_name, enum_parse};
use crate::search::{index_clip, truncate_body};
use crate::{Storage, StorageError};
use darkclip_core::{
    ClipId, Collection, CollectionId, CollectionKind, CollectionRef, CollectionSort, CoreError,
    Mime, SchemaVersion, SystemCollection, Tag, TagId, TagRef, Timestamp, NOTE_MAX_CHARS,
    TAGS_PER_CLIP_MAX,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;

/// A tag with the number of clips that carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub tag: Tag,
    pub clip_count: u64,
}

const SYSTEM_COLLECTION: StorageError =
    StorageError::InvalidArgument("system collection cannot be changed");
const NOT_MANUAL: StorageError = StorageError::InvalidArgument("collection is not manually sorted");
const MASKED: StorageError =
    StorageError::InvalidArgument("masked clips cannot have notes or tags");
const TOO_MANY_TAGS: StorageError = StorageError::InvalidArgument("too many tags");

/// `(inline_data, blob_sha256)` of a representation row.
type RepBytes = (Option<Vec<u8>>, Option<[u8; 32]>);

fn to_u64(n: i64) -> Result<u64, StorageError> {
    u64::try_from(n).map_err(|_| decode_err())
}

/// `(kind, sort)` of a collection, or `None` when it does not exist.
fn collection_meta(
    conn: &Connection,
    id: &str,
) -> Result<Option<(CollectionKind, CollectionSort)>, StorageError> {
    let row = conn
        .query_row(
            "SELECT kind, sort FROM collections WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    match row {
        Some((k, s)) => Ok(Some((enum_parse(k)?, enum_parse(s)?))),
        None => Ok(None),
    }
}

/// Whether the clip is masked, or `None` when it does not exist.
fn clip_masked(conn: &Connection, id: &str) -> Result<Option<bool>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT sensitivity_level = 'masked' FROM clips WHERE id = ?1",
            [id],
            |r| r.get::<_, bool>(0),
        )
        .optional()?)
}

/// Member clip ids in stored order.
fn member_ids(conn: &Connection, col: &str) -> Result<Vec<String>, StorageError> {
    let mut st = conn.prepare_cached(
        "SELECT clip_id FROM collection_clips WHERE collection_id = ?1 \
         ORDER BY ord, added_at, clip_id",
    )?;
    let rows = st.query_map([col], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for id in rows {
        out.push(id?);
    }
    Ok(out)
}

/// Writes `ord = index` for `ids` (the collection's full member list, in order).
fn write_order(conn: &Connection, col: &str, ids: &[String]) -> Result<(), StorageError> {
    let mut st = conn.prepare_cached(
        "UPDATE collection_clips SET ord = ?3 WHERE collection_id = ?1 AND clip_id = ?2",
    )?;
    for (i, id) in ids.iter().enumerate() {
        let ord = i64::try_from(i).map_err(|_| decode_err())?;
        st.execute(params![col, id, ord])?;
    }
    Ok(())
}

fn touch_collection(conn: &Connection, col: &str, now: Timestamp) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE collections SET updated_at = ?2 WHERE id = ?1",
        params![col, now.0],
    )?;
    Ok(())
}

/// Deletes every tag no clip carries. Called inside each transaction that can drop one.
pub(crate) fn delete_unused_tags(conn: &Connection) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM tags WHERE NOT EXISTS (SELECT 1 FROM clip_tags ct WHERE ct.tag_id = tags.id)",
        [],
    )?;
    Ok(())
}

/// The clip's tag names joined by spaces (the FTS `tags` column), sorted.
pub(crate) fn tags_text(conn: &Connection, clip: &str) -> Result<String, StorageError> {
    let mut st = conn.prepare_cached(
        "SELECT t.name FROM clip_tags ct JOIN tags t ON t.id = ct.tag_id \
         WHERE ct.clip_id = ?1 ORDER BY t.name",
    )?;
    let rows = st.query_map([clip], |r| r.get::<_, String>(0))?;
    let mut names = Vec::new();
    for n in rows {
        names.push(n?);
    }
    Ok(names.join(" "))
}

/// The clip's tags and collections, for `get_clip`.
pub(crate) fn clip_refs(
    conn: &Connection,
    clip: &str,
) -> Result<(Vec<TagRef>, Vec<CollectionRef>), StorageError> {
    let mut tags = Vec::new();
    {
        let mut st = conn.prepare_cached(
            "SELECT t.id, t.name FROM clip_tags ct JOIN tags t ON t.id = ct.tag_id \
             WHERE ct.clip_id = ?1 ORDER BY t.name",
        )?;
        let rows = st.query_map([clip], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, name) = row?;
            tags.push(TagRef {
                id: TagId::parse(&id).map_err(|_| decode_err())?,
                name,
            });
        }
    }
    let mut cols = Vec::new();
    {
        let mut st = conn.prepare_cached(
            "SELECT k.id, k.name FROM collection_clips m JOIN collections k \
             ON k.id = m.collection_id WHERE m.clip_id = ?1 \
             ORDER BY (k.kind = 'system') DESC, k.created_at, k.id",
        )?;
        let rows = st.query_map([clip], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, name) = row?;
            cols.push(CollectionRef {
                id: CollectionId::parse(&id).map_err(|_| decode_err())?,
                name,
            });
        }
    }
    Ok((tags, cols))
}

/// The plain text the FTS `body` column indexes: the first text/plain representation,
/// inline or from its blob (empty when there is none).
fn body_text(conn: &Connection, dir: &Path, clip: &str) -> Result<String, StorageError> {
    let row: Option<RepBytes> = conn
        .query_row(
            "SELECT inline_data, blob_sha256 FROM representations \
             WHERE clip_id = ?1 AND mime = ?2 ORDER BY ord LIMIT 1",
            params![clip, enum_name(&Mime::TextPlain)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let bytes = match row {
        Some((Some(bytes), _)) => bytes,
        Some((None, Some(sha))) => blobs::read(dir, &sha)?,
        Some((None, None)) => return Err(decode_err()),
        None => return Ok(String::new()),
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Rebuilds the clip's FTS row from the stored values (contentless table: delete, then
/// insert every column). A masked clip indexes its masked preview only.
fn reindex(conn: &Connection, dir: &Path, clip: &str) -> Result<(), StorageError> {
    let (preview, masked, note): (String, bool, Option<String>) = conn.query_row(
        "SELECT preview, sensitivity_level = 'masked', note FROM clips WHERE id = ?1",
        [clip],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if masked {
        return index_clip(conn, clip, &preview, "", "", "");
    }
    let body = body_text(conn, dir, clip)?;
    let tags = tags_text(conn, clip)?;
    index_clip(
        conn,
        clip,
        &preview,
        truncate_body(&body),
        note.as_deref().unwrap_or_default(),
        &tags,
    )
}

impl Storage {
    /// The id of the system Favorites collection.
    pub fn favorites_id(&self) -> Result<CollectionId, StorageError> {
        let id: String = self.conn.query_row(
            "SELECT id FROM collections WHERE system_key = ?1",
            [enum_name(&SystemCollection::Favorites)?],
            |r| r.get(0),
        )?;
        CollectionId::parse(&id).map_err(|_| decode_err())
    }

    /// Whether the clip is in Favorites, and its tag names (sorted). For list views.
    pub fn clip_labels(&self, clip: &ClipId) -> Result<(bool, Vec<String>), StorageError> {
        let clip_s = clip.to_string();
        let fav: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM collection_clips m JOIN collections k              ON k.id = m.collection_id WHERE m.clip_id = ?1 AND k.system_key = 'favorites')",
            [&clip_s],
            |r| r.get(0),
        )?;
        let tags = tags_text(&self.conn, &clip_s)?;
        Ok((fav, tags.split_whitespace().map(str::to_owned).collect()))
    }

    /// Creates a user collection. The name is trimmed (1-80 characters); `color` must be
    /// `#rrggbb` in lowercase hex.
    pub fn create_collection(
        &mut self,
        name: &str,
        sort: CollectionSort,
        color: Option<&str>,
    ) -> Result<Collection, StorageError> {
        self.ensure_writable()?;
        let name = Collection::validate_name(name)?;
        if let Some(c) = color {
            Collection::validate_color(c)?;
        }
        let now = self.clock.now();
        let id = CollectionId::new_v7(now);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO collections (id, name, kind, color, sort, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id.to_string(),
                name,
                enum_name(&CollectionKind::User)?,
                color,
                enum_name(&sort)?,
                now.0
            ],
        )?;
        tx.commit()?;
        Ok(Collection {
            schema_version: SchemaVersion,
            id,
            name,
            kind: CollectionKind::User,
            system_key: None,
            color: color.map(str::to_owned),
            created_at: now,
            updated_at: now,
            clip_count: 0,
            sort,
        })
    }

    pub fn rename_collection(&mut self, id: &CollectionId, name: &str) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let name = Collection::validate_name(name)?;
        let now = self.clock.now();
        let id_s = id.to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match collection_meta(&tx, &id_s)? {
            None => return Err(StorageError::NotFound),
            Some((CollectionKind::System, _)) => return Err(SYSTEM_COLLECTION),
            Some((CollectionKind::User, _)) => {}
        }
        tx.execute(
            "UPDATE collections SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id_s, name, now.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes a user collection and its memberships; the clips stay.
    pub fn delete_collection(&mut self, id: &CollectionId) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let id_s = id.to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match collection_meta(&tx, &id_s)? {
            None => return Err(StorageError::NotFound),
            Some((CollectionKind::System, _)) => return Err(SYSTEM_COLLECTION),
            Some((CollectionKind::User, _)) => {}
        }
        tx.execute("DELETE FROM collections WHERE id = ?1", [&id_s])?;
        tx.commit()?;
        Ok(())
    }

    /// Every collection, system first, then by creation; `clip_count` is derived.
    pub fn list_collections(&self) -> Result<Vec<Collection>, StorageError> {
        let mut st = self.conn.prepare_cached(
            "SELECT k.id, k.name, k.kind, k.system_key, k.color, k.sort, k.created_at, \
             k.updated_at, (SELECT count(*) FROM collection_clips m WHERE m.collection_id = k.id) \
             FROM collections k ORDER BY (k.kind = 'system') DESC, k.created_at, k.id",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let id: String = r.get(0)?;
            let system_key: Option<String> = r.get(3)?;
            out.push(Collection {
                schema_version: SchemaVersion,
                id: CollectionId::parse(&id).map_err(|_| decode_err())?,
                name: r.get(1)?,
                kind: enum_parse(r.get(2)?)?,
                system_key: system_key.map(enum_parse).transpose()?,
                color: r.get(4)?,
                sort: enum_parse(r.get(5)?)?,
                created_at: Timestamp(r.get(6)?),
                updated_at: Timestamp(r.get(7)?),
                clip_count: to_u64(r.get(8)?)?,
            });
        }
        Ok(out)
    }

    /// Adds the clip at the end of the collection. Adding a member again changes nothing.
    pub fn add_to_collection(
        &mut self,
        col: &CollectionId,
        clip: &ClipId,
    ) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let (col_s, clip_s) = (col.to_string(), clip.to_string());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if collection_meta(&tx, &col_s)?.is_none() || clip_masked(&tx, &clip_s)?.is_none() {
            return Err(StorageError::NotFound);
        }
        let added = tx.execute(
            "INSERT OR IGNORE INTO collection_clips (collection_id, clip_id, ord, added_at) \
             VALUES (?1, ?2, (SELECT COALESCE(MAX(ord) + 1, 0) FROM collection_clips \
             WHERE collection_id = ?1), ?3)",
            params![col_s, clip_s, now.0],
        )?;
        if added > 0 {
            touch_collection(&tx, &col_s, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes the clip from the collection; removing a non-member changes nothing.
    pub fn remove_from_collection(
        &mut self,
        col: &CollectionId,
        clip: &ClipId,
    ) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let (col_s, clip_s) = (col.to_string(), clip.to_string());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if collection_meta(&tx, &col_s)?.is_none() {
            return Err(StorageError::NotFound);
        }
        let removed = tx.execute(
            "DELETE FROM collection_clips WHERE collection_id = ?1 AND clip_id = ?2",
            params![col_s, clip_s],
        )?;
        if removed > 0 {
            let ids = member_ids(&tx, &col_s)?;
            write_order(&tx, &col_s, &ids)?;
            touch_collection(&tx, &col_s, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves a member of a `manual` collection to `index` (clamped to the end) and
    /// renumbers the collection densely.
    pub fn move_in_collection(
        &mut self,
        col: &CollectionId,
        clip: &ClipId,
        index: u32,
    ) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let (col_s, clip_s) = (col.to_string(), clip.to_string());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match collection_meta(&tx, &col_s)? {
            None => return Err(StorageError::NotFound),
            Some((_, CollectionSort::Recent)) => return Err(NOT_MANUAL),
            Some((_, CollectionSort::Manual)) => {}
        }
        let mut ids = member_ids(&tx, &col_s)?;
        let Some(from) = ids.iter().position(|id| *id == clip_s) else {
            return Err(StorageError::NotFound);
        };
        let moved = ids.remove(from);
        let to = usize::try_from(index).unwrap_or(usize::MAX).min(ids.len());
        ids.insert(to, moved);
        write_order(&tx, &col_s, &ids)?;
        touch_collection(&tx, &col_s, now)?;
        tx.commit()?;
        Ok(())
    }

    /// The collection's clip ids in stored order.
    pub fn collection_clip_ids(&self, col: &CollectionId) -> Result<Vec<ClipId>, StorageError> {
        let col_s = col.to_string();
        if collection_meta(&self.conn, &col_s)?.is_none() {
            return Err(StorageError::NotFound);
        }
        member_ids(&self.conn, &col_s)?
            .iter()
            .map(|id| ClipId::parse(id).map_err(|_| decode_err()))
            .collect()
    }

    /// Replaces the clip's tags with `names` (normalised, duplicates merged, at most 20).
    /// Returns the stored tags, sorted by name. Tags left unused are deleted.
    pub fn set_clip_tags<S: AsRef<str>>(
        &mut self,
        clip: &ClipId,
        names: &[S],
    ) -> Result<Vec<TagRef>, StorageError> {
        self.ensure_writable()?;
        // Stop at the first name past the limit: the input length is not bounded.
        let mut normal: Vec<String> = Vec::with_capacity(TAGS_PER_CLIP_MAX);
        for n in names {
            let name = Tag::validate_name(n.as_ref())?;
            if !normal.contains(&name) {
                if normal.len() == TAGS_PER_CLIP_MAX {
                    return Err(TOO_MANY_TAGS);
                }
                normal.push(name);
            }
        }
        let now = self.clock.now();
        let clip_s = clip.to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match clip_masked(&tx, &clip_s)? {
            None => return Err(StorageError::NotFound),
            Some(true) if !normal.is_empty() => return Err(MASKED),
            Some(_) => {}
        }
        tx.execute("DELETE FROM clip_tags WHERE clip_id = ?1", [&clip_s])?;
        {
            let mut add_tag = tx.prepare_cached(
                "INSERT OR IGNORE INTO tags (id, name, created_at) VALUES (?1, ?2, ?3)",
            )?;
            let mut link = tx.prepare_cached(
                "INSERT INTO clip_tags (clip_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2",
            )?;
            for name in &normal {
                add_tag.execute(params![TagId::new_v7(now).to_string(), name, now.0])?;
                link.execute(params![clip_s, name])?;
            }
        }
        delete_unused_tags(&tx)?;
        reindex(&tx, &self.dir, &clip_s)?;
        let (tags, _) = clip_refs(&tx, &clip_s)?;
        tx.commit()?;
        Ok(tags)
    }

    /// Every tag with its clip count, sorted by name.
    pub fn list_tags(&self) -> Result<Vec<TagCount>, StorageError> {
        let mut st = self.conn.prepare_cached(
            "SELECT t.id, t.name, t.created_at, \
             (SELECT count(*) FROM clip_tags ct WHERE ct.tag_id = t.id) \
             FROM tags t ORDER BY t.name",
        )?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let id: String = r.get(0)?;
            out.push(TagCount {
                tag: Tag {
                    schema_version: SchemaVersion,
                    id: TagId::parse(&id).map_err(|_| decode_err())?,
                    name: r.get(1)?,
                    created_at: Timestamp(r.get(2)?),
                },
                clip_count: to_u64(r.get(3)?)?,
            });
        }
        Ok(out)
    }

    /// Sets the clip's note (at most 4,000 characters); `None` or an empty note clears it.
    pub fn set_note(&mut self, clip: &ClipId, note: Option<&str>) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let note = note.filter(|n| !n.is_empty());
        if note.is_some_and(|n| n.chars().count() > NOTE_MAX_CHARS) {
            return Err(StorageError::Invalid(CoreError::TooLong {
                field: "note",
                max: NOTE_MAX_CHARS,
            }));
        }
        let clip_s = clip.to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match clip_masked(&tx, &clip_s)? {
            None => return Err(StorageError::NotFound),
            Some(true) if note.is_some() => return Err(MASKED),
            Some(_) => {}
        }
        tx.execute(
            "UPDATE clips SET note = ?2 WHERE id = ?1",
            params![clip_s, note],
        )?;
        reindex(&tx, &self.dir, &clip_s)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
