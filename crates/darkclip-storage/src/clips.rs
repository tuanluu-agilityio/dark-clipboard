//! Insert/dedupe, get, representation reads, use and pin bookkeeping.
//!
//! Mapping `NewClip` -> stored `Clip`:
//! - `kind`, `preview` and `metadata` come from `classified` (the caller has already
//!   replaced the preview with the masked one for masked clips);
//!   `metadata.image_thumb_sha256` is set here from `thumbnail_png`.
//! - One `Representation` per raw rep, in order (`ord` = index). Reps larger than
//!   64 KiB and every image go to a blob file; the rest are inline.
//! - `size_bytes` is the sum of the representation sizes.
//! - `classified.plain_text` is not stored separately: it is the text/plain rep.
//!
//! On a `content_hash` conflict the existing row keeps its id, `created_at`, use
//! counters, pin state and note; it gets `last_copied_at = now`, `copy_count + 1`,
//! the new source, and everything derived from the new capture (representations,
//! preview, size, metadata/thumbnail, sensitivity, `expires_at`).

use crate::blobs;
use crate::{NewClip, Storage, StorageError, UpsertOutcome};
use darkclip_core::{
    Clip, ClipId, ClipMetadata, ClipType, ClipboardSource, Mime, Platform, Representation,
    SchemaVersion, SensitiveCategory, Sensitivity, SensitivityLevel, SourceConfidence,
    Storage as RepStorage, Timestamp,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Larger representations go to blob files (ADR-0003: text <= 64 KiB inline).
const INLINE_MAX_BYTES: usize = 64 * 1024;
/// ADR-0003: thumbnails are at most 256 px.
const THUMB_MAX_PX: u32 = 256;

type InlineData = Option<Vec<u8>>;
type BlobRef = Option<[u8; 32]>;

fn decode_err() -> StorageError {
    StorageError::Sqlite("ColumnDecode".into())
}

/// The serde name of a unit enum variant (`"rich_text"`, `"linux-x11"`, ...).
fn enum_name<T: Serialize>(v: &T) -> Result<String, StorageError> {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(StorageError::Sqlite("ColumnEncode".into())),
    }
}

fn enum_parse<T: DeserializeOwned>(s: String) -> Result<T, StorageError> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|_| decode_err())
}

fn to_i64(n: u64) -> Result<i64, StorageError> {
    i64::try_from(n).map_err(|_| StorageError::Sqlite("IntegerOverflow".into()))
}

fn to_u64(n: i64) -> Result<u64, StorageError> {
    u64::try_from(n).map_err(|_| decode_err())
}

fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Accepts the thumbnail only if it is a PNG within the size bound (header check).
fn valid_thumbnail(png_bytes: &[u8]) -> bool {
    let mut d = png::Decoder::new(std::io::Cursor::new(png_bytes));
    match d.read_header_info() {
        Ok(info) => {
            (1..=THUMB_MAX_PX).contains(&info.width) && (1..=THUMB_MAX_PX).contains(&info.height)
        }
        Err(_) => false,
    }
}

/// Row values of an existing clip that a bump keeps.
struct Existing {
    id: ClipId,
    created_at: Timestamp,
    copy_count: u64,
    use_count: u64,
    last_used_at: Option<Timestamp>,
    pinned: bool,
    pin_order: Option<u32>,
    note: Option<String>,
    old_blobs: Vec<[u8; 32]>,
}

/// Representation rows ready to insert, with the bytes for inline ones.
struct PreparedRep<'a> {
    rep: Representation,
    inline: Option<&'a [u8]>,
    bytes: &'a [u8],
}

impl Storage {
    /// Inserts or, on content_hash conflict, bumps last_copied_at/copy_count/source and replaces representations.
    pub fn upsert_clip(&mut self, new: &NewClip) -> Result<UpsertOutcome, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing = find_by_hash(&tx, &new.hash)?;

        let reps: Vec<PreparedRep<'_>> = new
            .reps
            .iter()
            .map(|r| {
                let blob = r.bytes.len() > INLINE_MAX_BYTES || r.mime == Mime::ImagePng;
                PreparedRep {
                    rep: Representation {
                        mime: r.mime,
                        platform_format: Some(r.platform_format.clone()).filter(|s| !s.is_empty()),
                        size_bytes: len_u64(r.bytes.len()),
                        storage: if blob {
                            RepStorage::Blob
                        } else {
                            RepStorage::Inline
                        },
                        blob_sha256: blob.then(|| blobs::sha256(&r.bytes)),
                        width: r.width.filter(|w| *w > 0),
                        height: r.height.filter(|h| *h > 0),
                    },
                    inline: (!blob).then_some(r.bytes.as_slice()),
                    bytes: &r.bytes,
                }
            })
            .collect();
        let thumb = new
            .thumbnail_png
            .as_deref()
            .filter(|t| valid_thumbnail(t))
            .map(|t| (blobs::sha256(t), t));

        let mut metadata = new.classified.metadata.clone();
        metadata.image_thumb_sha256 = thumb.as_ref().map(|(sha, _)| *sha);
        let clip = Clip {
            schema_version: SchemaVersion,
            id: existing
                .as_ref()
                .map_or_else(|| ClipId::new_v7(now), |e| e.id),
            kind: new.classified.kind,
            preview: new.classified.preview.clone(),
            representations: reps.iter().map(|p| p.rep.clone()).collect(),
            content_hash: new.hash,
            size_bytes: reps.iter().map(|p| p.rep.size_bytes).sum(),
            created_at: existing.as_ref().map_or(now, |e| e.created_at),
            last_copied_at: now,
            last_used_at: existing.as_ref().and_then(|e| e.last_used_at),
            copy_count: existing
                .as_ref()
                .map_or(1, |e| e.copy_count.saturating_add(1)),
            use_count: existing.as_ref().map_or(0, |e| e.use_count),
            pinned: existing.as_ref().is_some_and(|e| e.pinned),
            pin_order: existing.as_ref().and_then(|e| e.pin_order),
            source: new.source.clone(),
            sensitivity: new.sensitivity.clone(),
            note: existing.as_ref().and_then(|e| e.note.clone()),
            tags: Vec::new(),
            collections: Vec::new(),
            expires_at: new.expires_at,
            metadata,
        };
        clip.validate()?;

        // Blob files first: they must be on disk before the rows that name them commit.
        for p in &reps {
            if let Some(sha) = &p.rep.blob_sha256 {
                blobs::write(&self.dir, sha, p.bytes)?;
            }
        }
        if let Some((sha, bytes)) = &thumb {
            blobs::write(&self.dir, sha, bytes)?;
        }

        let id_s = clip.id.to_string();
        let kind = enum_name(&clip.kind)?;
        let (src_id, src_name, src_platform, src_conf) = match &clip.source {
            Some(s) => (
                Some(s.app_id.clone()),
                s.app_name.clone(),
                Some(enum_name(&s.platform)?),
                Some(enum_name(&s.confidence)?),
            ),
            None => (None, None, None, None),
        };
        let level = enum_name(&clip.sensitivity.level)?;
        let categories = if clip.sensitivity.categories.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&clip.sensitivity.categories)
                    .map_err(|_| StorageError::Sqlite("ColumnEncode".into()))?,
            )
        };
        let meta = serde_json::to_string(&clip.metadata)
            .map_err(|_| StorageError::Sqlite("ColumnEncode".into()))?;
        let size = to_i64(clip.size_bytes)?;
        let expires = clip.expires_at.map(|t| t.0);

        let mut stale = Vec::new();
        if let Some(e) = &existing {
            stale.extend(e.old_blobs.iter().copied());
            tx.execute("DELETE FROM representations WHERE clip_id = ?1", [&id_s])?;
            tx.execute(
                "UPDATE clips SET preview = ?2, size_bytes = ?3, last_copied_at = ?4, \
                 copy_count = copy_count + 1, source_app_id = ?5, source_app_name = ?6, \
                 source_platform = ?7, source_confidence = ?8, sensitivity_level = ?9, \
                 sensitivity_categories = ?10, expires_at = ?11, meta = ?12 WHERE id = ?1",
                params![
                    id_s,
                    clip.preview,
                    size,
                    now.0,
                    src_id,
                    src_name,
                    src_platform,
                    src_conf,
                    level,
                    categories,
                    expires,
                    meta
                ],
            )?;
        } else {
            tx.execute(
                "INSERT INTO clips (id, kind, preview, content_hash, size_bytes, created_at, \
                 last_copied_at, copy_count, use_count, pinned, source_app_id, \
                 source_app_name, source_platform, source_confidence, sensitivity_level, \
                 sensitivity_categories, expires_at, meta) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1, 0, 0, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    id_s,
                    kind,
                    clip.preview,
                    clip.content_hash,
                    size,
                    now.0,
                    src_id,
                    src_name,
                    src_platform,
                    src_conf,
                    level,
                    categories,
                    expires,
                    meta
                ],
            )?;
            tx.execute("INSERT INTO clip_rowid (clip_id) VALUES (?1)", [&id_s])?;
        }
        {
            let mut ins = tx.prepare(
                "INSERT INTO representations (clip_id, ord, mime, platform_format, size_bytes, \
                 inline_data, blob_sha256, width, height) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for (ord, p) in reps.iter().enumerate() {
                ins.execute(params![
                    id_s,
                    i64::try_from(ord).map_err(|_| decode_err())?,
                    enum_name(&p.rep.mime)?,
                    p.rep.platform_format,
                    to_i64(p.rep.size_bytes)?,
                    p.inline,
                    p.rep.blob_sha256,
                    p.rep.width,
                    p.rep.height
                ])?;
            }
        }

        // FTS row: masked clips index only the masked preview, never body or note.
        let masked = clip.sensitivity.level == SensitivityLevel::Masked;
        let body = if masked {
            String::new()
        } else {
            new.reps
                .iter()
                .find(|r| r.mime == Mime::TextPlain)
                .map(|r| String::from_utf8_lossy(&r.bytes).into_owned())
                .unwrap_or_default()
        };
        let note = clip.note.as_deref().filter(|_| !masked).unwrap_or_default();
        crate::search::index_clip(
            &tx,
            &id_s,
            &clip.preview,
            crate::search::truncate_body(&body),
            note,
        )?;

        #[cfg(test)]
        if self.fail_before_commit {
            return Err(StorageError::Io(std::io::ErrorKind::Interrupted));
        }
        tx.commit()?;

        // After commit: unlink blobs the old representations or thumbnail used and
        // nothing references any more (under BEGIN IMMEDIATE). A failure here only
        // leaves an orphan for the next sweep.
        if !stale.is_empty() {
            let _ = blobs::remove_unreferenced(&mut self.conn, &self.dir, &stale);
        }
        Ok(match existing {
            Some(_) => UpsertOutcome::Bumped(clip.id),
            None => UpsertOutcome::Inserted(clip.id),
        })
    }

    pub fn get_clip(&self, id: &ClipId) -> Result<Option<Clip>, StorageError> {
        let id_s = id.to_string();
        let row = self
            .conn
            .query_row(
                "SELECT kind, preview, content_hash, size_bytes, created_at, last_copied_at, \
                 last_used_at, copy_count, use_count, pinned, pin_order, source_app_id, \
                 source_app_name, source_platform, source_confidence, sensitivity_level, \
                 sensitivity_categories, note, expires_at, meta FROM clips WHERE id = ?1",
                [&id_s],
                |r| {
                    Ok(ClipRow {
                        kind: r.get(0)?,
                        preview: r.get(1)?,
                        content_hash: r.get(2)?,
                        size_bytes: r.get(3)?,
                        created_at: r.get(4)?,
                        last_copied_at: r.get(5)?,
                        last_used_at: r.get(6)?,
                        copy_count: r.get(7)?,
                        use_count: r.get(8)?,
                        pinned: r.get(9)?,
                        pin_order: r.get(10)?,
                        src_id: r.get(11)?,
                        src_name: r.get(12)?,
                        src_platform: r.get(13)?,
                        src_conf: r.get(14)?,
                        level: r.get(15)?,
                        categories: r.get(16)?,
                        note: r.get(17)?,
                        expires_at: r.get(18)?,
                        meta: r.get(19)?,
                    })
                },
            )
            .optional()?;
        let Some(row) = row else {
            return Ok(None);
        };

        let mut st = self.conn.prepare(
            "SELECT mime, platform_format, size_bytes, blob_sha256, width, height \
             FROM representations WHERE clip_id = ?1 ORDER BY ord",
        )?;
        let raw_reps = st
            .query_map([&id_s], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<[u8; 32]>>(3)?,
                    r.get::<_, Option<u32>>(4)?,
                    r.get::<_, Option<u32>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut representations = Vec::with_capacity(raw_reps.len());
        for (mime, platform_format, size, blob, width, height) in raw_reps {
            representations.push(Representation {
                mime: enum_parse::<Mime>(mime)?,
                platform_format,
                size_bytes: to_u64(size)?,
                storage: if blob.is_some() {
                    RepStorage::Blob
                } else {
                    RepStorage::Inline
                },
                blob_sha256: blob,
                width,
                height,
            });
        }

        let source = match (row.src_id, row.src_platform, row.src_conf) {
            (Some(app_id), Some(p), Some(c)) => Some(ClipboardSource {
                app_id,
                app_name: row.src_name,
                platform: enum_parse::<Platform>(p)?,
                confidence: enum_parse::<SourceConfidence>(c)?,
            }),
            _ => None,
        };
        let categories: Vec<SensitiveCategory> = match row.categories {
            Some(json) => serde_json::from_str(&json).map_err(|_| decode_err())?,
            None => Vec::new(),
        };
        let metadata: ClipMetadata = serde_json::from_str(&row.meta).map_err(|_| decode_err())?;
        Ok(Some(Clip {
            schema_version: SchemaVersion,
            id: *id,
            kind: enum_parse::<ClipType>(row.kind)?,
            preview: row.preview,
            representations,
            content_hash: row.content_hash,
            size_bytes: to_u64(row.size_bytes)?,
            created_at: Timestamp(row.created_at),
            last_copied_at: Timestamp(row.last_copied_at),
            last_used_at: row.last_used_at.map(Timestamp),
            copy_count: to_u64(row.copy_count)?,
            use_count: to_u64(row.use_count)?,
            pinned: row.pinned,
            pin_order: row.pin_order,
            source,
            sensitivity: Sensitivity {
                level: enum_parse::<SensitivityLevel>(row.level)?,
                categories,
            },
            note: row.note,
            tags: Vec::new(),
            collections: Vec::new(),
            expires_at: row.expires_at.map(Timestamp),
            metadata,
        }))
    }

    /// The bytes of representation `ord`, from the row (inline) or its blob file.
    ///
    /// Reader race: another connection (the writer) may delete the clip, or replace
    /// its representations on a bump, and unlink the blob between this connection
    /// reading the row and opening the file. That surfaces as a transient
    /// `StorageError::Io(ErrorKind::NotFound)`; callers treat it like a missing clip.
    pub fn read_representation(&self, id: &ClipId, ord: u32) -> Result<Vec<u8>, StorageError> {
        let row: Option<(InlineData, BlobRef)> = self
            .conn
            .query_row(
                "SELECT inline_data, blob_sha256 FROM representations \
                 WHERE clip_id = ?1 AND ord = ?2",
                params![id.to_string(), ord],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((Some(bytes), _)) => Ok(bytes),
            Some((None, Some(sha))) => blobs::read(&self.dir, &sha),
            Some((None, None)) => Err(decode_err()),
            None => Err(StorageError::NotFound),
        }
    }

    pub fn mark_used(&mut self, id: &ClipId) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let n = self.conn.execute(
            "UPDATE clips SET last_used_at = ?2, use_count = use_count + 1 WHERE id = ?1",
            params![id.to_string(), now.0],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Pinning appends the clip to the end of the pin order; unpinning clears it.
    pub fn set_pinned(&mut self, id: &ClipId, pinned: bool) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let sql = if pinned {
            "UPDATE clips SET pinned = 1, pin_order = COALESCE(pin_order, \
             (SELECT COALESCE(MAX(pin_order) + 1, 0) FROM clips)) WHERE id = ?1"
        } else {
            "UPDATE clips SET pinned = 0, pin_order = NULL WHERE id = ?1"
        };
        if self.conn.execute(sql, [id.to_string()])? == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }
}

struct ClipRow {
    kind: String,
    preview: String,
    content_hash: [u8; 32],
    size_bytes: i64,
    created_at: i64,
    last_copied_at: i64,
    last_used_at: Option<i64>,
    copy_count: i64,
    use_count: i64,
    pinned: bool,
    pin_order: Option<u32>,
    src_id: Option<String>,
    src_name: Option<String>,
    src_platform: Option<String>,
    src_conf: Option<String>,
    level: String,
    categories: Option<String>,
    note: Option<String>,
    expires_at: Option<i64>,
    meta: String,
}

fn find_by_hash(
    conn: &rusqlite::Connection,
    hash: &[u8; 32],
) -> Result<Option<Existing>, StorageError> {
    let row = conn
        .query_row(
            "SELECT id, created_at, copy_count, use_count, last_used_at, pinned, pin_order, \
             note, json_extract(meta, '$.image_thumb_sha256') FROM clips WHERE content_hash = ?1",
            [hash],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, Option<u32>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?;
    let Some((id, created, copies, uses, used_at, pinned, pin_order, note, thumb)) = row else {
        return Ok(None);
    };
    let id = ClipId::parse(&id).map_err(|_| decode_err())?;
    let mut old_blobs = Vec::new();
    {
        let mut st = conn.prepare(
            "SELECT blob_sha256 FROM representations \
             WHERE clip_id = ?1 AND blob_sha256 IS NOT NULL",
        )?;
        for sha in st.query_map([id.to_string()], |r| r.get::<_, [u8; 32]>(0))? {
            old_blobs.push(sha?);
        }
    }
    if let Some(h) = thumb {
        if let Some(sha) = parse_hex32(&h) {
            old_blobs.push(sha);
        }
    }
    Ok(Some(Existing {
        id,
        created_at: Timestamp(created),
        copy_count: to_u64(copies)?,
        use_count: to_u64(uses)?,
        last_used_at: used_at.map(Timestamp),
        pinned,
        pin_order,
        note,
        old_blobs,
    }))
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}
