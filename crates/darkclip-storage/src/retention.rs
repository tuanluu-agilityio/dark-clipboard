//! Deletion, clear history, retention and orphan-blob sweep (data-model §3, §5, §6).
//!
//! Rules:
//! - Only `clips` rows are deleted; the FK cascade removes representations and
//!   `clip_rowid`, whose trigger removes the FTS row. `secure_delete` is ON.
//! - Blob files are unlinked only after the transaction commits, and only when no
//!   row references them (`blobs::remove_unreferenced`, under `BEGIN IMMEDIATE`).
//!   A failed unlink is left for the next sweep.
//! - After deleting more than 100 rows (and always after `clear_history`) the WAL is
//!   truncated so deleted content does not linger in it.
//! - Retention order: expired -> age -> count -> size. Pinned clips are never
//!   evicted by age, count or size (`keep_pinned` is therefore not consulted).
//!   Favorites (members of the system collection `favorites`) are exempt the same way
//!   while `keep_favorites` is true. Expired clips (`expires_at <= now`) are deleted even
//!   when pinned or favorite: the sensitive TTL is a privacy promise (D-025).
//!   A limit of 0 (`max_items`, `max_age_days`, `max_storage_mb`) means no limit.
//! - Size = `Storage::db_bytes` + `Storage::blob_bytes` (see `stats.rs`). Victims are
//!   the oldest unpinned clips by `last_copied_at`; the amount freed is estimated
//!   from `size_bytes`, then re-measured, and the loop repeats until under the limit
//!   or only pinned (and kept favorite) clips remain.
//! - Deleting a clip cascades its collection memberships and tags; tags no clip uses any
//!   more are deleted in the same transaction.
//! - `bytes_freed` is the sum of the deleted clips' `size_bytes` (logical bytes; a
//!   blob shared with a surviving clip is counted though its file stays).

use crate::blobs::{self, Cleanup};
use crate::{ClipId, Storage, StorageError};
use darkclip_core::{RetentionPolicy, Timestamp};
use rusqlite::{Connection, Transaction, TransactionBehavior};

const MS_PER_DAY: i64 = 86_400_000;
const CHECKPOINT_THRESHOLD: u64 = 100;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionReport {
    pub expired: u64,
    pub evicted_count: u64,
    pub evicted_age: u64,
    pub evicted_size: u64,
    pub bytes_freed: u64,
}

#[derive(Default)]
struct Deleted {
    count: u64,
    bytes: u64,
    shas: Vec<[u8; 32]>,
}

impl Deleted {
    fn add(&mut self, other: Deleted) {
        self.count += other.count;
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.shas.extend(other.shas);
    }
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn to_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// Deletes the given clip rows inside `tx` and returns what was removed. The blob
/// hashes are read before the delete (the cascade drops the representation rows).
fn delete_ids(tx: &Transaction<'_>, ids: &[String]) -> Result<Deleted, StorageError> {
    let mut out = Deleted::default();
    let mut reps = tx.prepare(
        "SELECT blob_sha256 FROM representations WHERE clip_id = ?1 AND blob_sha256 IS NOT NULL",
    )?;
    let mut meta = tx.prepare(
        "SELECT size_bytes, json_extract(meta, '$.image_thumb_sha256') FROM clips WHERE id = ?1",
    )?;
    let mut del = tx.prepare("DELETE FROM clips WHERE id = ?1")?;
    for id in ids {
        let row = match meta.query_row([id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        }) {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => continue,
            Err(e) => return Err(e.into()),
        };
        let (size, thumb) = row;
        let shas = reps.query_map([id], |r| r.get::<_, Vec<u8>>(0))?;
        for sha in shas {
            if let Ok(arr) = <[u8; 32]>::try_from(sha?) {
                out.shas.push(arr);
            }
        }
        if let Some(arr) = thumb.as_deref().and_then(parse_hex32) {
            out.shas.push(arr);
        }
        if del.execute([id])? > 0 {
            out.count += 1;
            out.bytes = out.bytes.saturating_add(to_u64(size));
        }
    }
    if out.count > 0 {
        crate::organize::delete_unused_tags(tx)?;
    }
    out.shas.sort_unstable();
    out.shas.dedup();
    Ok(out)
}

/// The extra `WHERE` clause that keeps members of the system Favorites collection out.
const NOT_FAVORITE: &str = " AND id NOT IN (SELECT m.clip_id FROM collection_clips m \
     JOIN collections k ON k.id = m.collection_id WHERE k.system_key = 'favorites')";

/// `NOT_FAVORITE` when the policy keeps favorites (age, count and size eviction).
fn favorites_exempt(p: &RetentionPolicy) -> &'static str {
    if p.keep_favorites {
        NOT_FAVORITE
    } else {
        ""
    }
}

/// The clips "clear history" deletes: all of them, or with `keep_pinned` the ones that are
/// neither pinned nor favorite.
fn clearable_sql(keep_pinned: bool) -> String {
    if keep_pinned {
        format!("SELECT id FROM clips WHERE pinned = 0{NOT_FAVORITE}")
    } else {
        "SELECT id FROM clips".to_owned()
    }
}

/// The clips "delete older than" deletes: unpinned, not favorite, last copied before `?1`.
fn older_than_sql(select: &str) -> String {
    format!("SELECT {select} FROM clips WHERE pinned = 0 AND last_copied_at < ?1{NOT_FAVORITE}")
}

fn cutoff(now: i64, days: u32) -> Result<i64, StorageError> {
    if days == 0 {
        return Err(StorageError::InvalidArgument("days must be 1 or more"));
    }
    Ok(now.saturating_sub(i64::from(days).saturating_mul(MS_PER_DAY)))
}

fn select_ids(
    tx: &Transaction<'_>,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<String>, StorageError> {
    let mut st = tx.prepare(sql)?;
    let rows = st.query_map(params, |r| r.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for id in rows {
        ids.push(id?);
    }
    Ok(ids)
}

pub(crate) fn checkpoint(conn: &Connection) {
    // Best-effort: a busy reader only delays the truncate.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

impl Storage {
    /// Post-commit cleanup shared by every deletion path.
    /// `user`: a deletion the user asked for merges the search index and truncates the WAL at
    /// once (F-20). Automatic evictions only mark the index dirty: a full merge costs 100 ms
    /// or more at 10k clips and would run after every capture; the actor merges on its tick.
    fn finish_delete(&mut self, d: &Deleted, force_checkpoint: bool, user: bool) {
        if !d.shas.is_empty() {
            let _ = blobs::remove_unreferenced(&mut self.conn, &self.dir, &d.shas);
        }
        // Contentless FTS5 keeps the tokens of deleted rows until its segments merge, and
        // neither `secure_delete` nor `VACUUM` removes them (F-20): merge, then truncate the
        // WAL so the old pages are gone from both files.
        if user {
            if d.count > 0 {
                self.fts_merge();
            }
            if force_checkpoint || d.count > 0 {
                checkpoint(&self.conn);
            }
            return;
        }
        if d.count > 0 {
            self.fts_dirty = self
                .fts_dirty
                .saturating_add(u32::try_from(d.count).unwrap_or(u32::MAX));
            if self.fts_dirty >= crate::maintenance::FTS_BATCH {
                self.optimize_fts_if_dirty();
            }
        }
        if force_checkpoint || d.count > CHECKPOINT_THRESHOLD {
            checkpoint(&self.conn);
        }
    }

    pub fn delete_clip(&mut self, id: &ClipId) -> Result<(), StorageError> {
        self.ensure_writable()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let d = delete_ids(&tx, &[id.to_string()])?;
        if d.count == 0 {
            return Err(StorageError::NotFound);
        }
        tx.commit()?;
        self.finish_delete(&d, false, true);
        Ok(())
    }

    /// Deletes every clip, or with `keep_pinned` every clip that is neither pinned nor a
    /// favorite. Returns the number deleted.
    pub fn clear_history(&mut self, keep_pinned: bool) -> Result<u64, StorageError> {
        self.ensure_writable()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = select_ids(&tx, &clearable_sql(keep_pinned), [])?;
        let d = delete_ids(&tx, &ids)?;
        tx.commit()?;
        self.finish_delete(&d, true, true);
        Ok(d.count)
    }

    pub fn apply_retention(
        &mut self,
        p: &RetentionPolicy,
    ) -> Result<RetentionReport, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now().0;
        let mut report = RetentionReport::default();
        let mut all = Deleted::default();

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // 1. expired
        let ids = select_ids(
            &tx,
            "SELECT id FROM clips WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let d = delete_ids(&tx, &ids)?;
        report.expired = d.count;
        all.add(d);
        // 2. age
        if p.max_age_days > 0 {
            let cutoff = now.saturating_sub(i64::from(p.max_age_days).saturating_mul(MS_PER_DAY));
            let ids = select_ids(
                &tx,
                &format!(
                    "SELECT id FROM clips WHERE pinned = 0 AND last_copied_at < ?1{}",
                    favorites_exempt(p)
                ),
                [cutoff],
            )?;
            let d = delete_ids(&tx, &ids)?;
            report.evicted_age = d.count;
            all.add(d);
        }
        // 3. count (pinned and kept favorites count toward the total but are never evicted)
        if p.max_items > 0 {
            let total: i64 = tx.query_row("SELECT count(*) FROM clips", [], |r| r.get(0))?;
            let excess = to_u64(total).saturating_sub(u64::from(p.max_items));
            if excess > 0 {
                let ids = select_ids(
                    &tx,
                    &format!(
                        "SELECT id FROM clips WHERE pinned = 0{} \
                         ORDER BY last_copied_at ASC, created_at ASC, id ASC LIMIT ?1",
                        favorites_exempt(p)
                    ),
                    [i64::try_from(excess).unwrap_or(i64::MAX)],
                )?;
                let d = delete_ids(&tx, &ids)?;
                report.evicted_count = d.count;
                all.add(d);
            }
        }
        tx.commit()?;
        self.finish_delete(&all, false, false);
        let mut deleted_total = all.count;
        let mut freed = all.bytes;

        // 4. size
        if p.max_storage_mb > 0 {
            let limit = u64::from(p.max_storage_mb).saturating_mul(1024 * 1024);
            loop {
                let total = self.db_bytes()?.saturating_add(self.blob_bytes()?);
                if total <= limit {
                    break;
                }
                let need = total - limit;
                let tx = self
                    .conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut ids = Vec::new();
                let mut est = 0u64;
                {
                    let mut st = tx.prepare(&format!(
                        "SELECT id, size_bytes FROM clips WHERE pinned = 0{} \
                         ORDER BY last_copied_at ASC, created_at ASC, id ASC",
                        favorites_exempt(p)
                    ))?;
                    let rows =
                        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                    for row in rows {
                        let (id, size) = row?;
                        ids.push(id);
                        est = est.saturating_add(to_u64(size));
                        if est >= need {
                            break;
                        }
                    }
                }
                if ids.is_empty() {
                    break; // only pinned (and kept favorite) clips remain
                }
                let d = delete_ids(&tx, &ids)?;
                tx.commit()?;
                report.evicted_size += d.count;
                freed = freed.saturating_add(d.bytes);
                deleted_total += d.count;
                self.finish_delete(&d, false, false);
            }
        }
        report.bytes_freed = freed;
        if deleted_total > CHECKPOINT_THRESHOLD {
            checkpoint(&self.conn);
        }
        Ok(report)
    }

    /// How many clips `clear_history(keep_pinned)` would delete.
    pub fn count_clearable(&self, keep_pinned: bool) -> Result<u64, StorageError> {
        let sql = clearable_sql(keep_pinned).replacen("SELECT id", "SELECT count(*)", 1);
        let n: i64 = self.conn.query_row(&sql, [], |r| r.get(0))?;
        Ok(to_u64(n))
    }

    /// How many clips `delete_older_than(days)` would delete.
    pub fn count_older_than(&self, days: u32) -> Result<u64, StorageError> {
        let cut = cutoff(self.clock.now().0, days)?;
        let n: i64 = self
            .conn
            .query_row(&older_than_sql("count(*)"), [cut], |r| r.get(0))?;
        Ok(to_u64(n))
    }

    /// Deletes the clips last copied more than `days` days ago, except pinned clips and
    /// favorites. `days` must be at least 1. Returns the number deleted.
    pub fn delete_older_than(&mut self, days: u32) -> Result<u64, StorageError> {
        self.ensure_writable()?;
        let cut = cutoff(self.clock.now().0, days)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = select_ids(&tx, &older_than_sql("id"), [cut])?;
        let d = delete_ids(&tx, &ids)?;
        tx.commit()?;
        self.finish_delete(&d, true, true);
        Ok(d.count)
    }

    /// Removes blob files (and `.tmp` leftovers) that nothing references. Returns the
    /// number removed.
    pub fn sweep_orphan_blobs(&mut self) -> Result<u64, StorageError> {
        self.ensure_writable()?;
        let Cleanup { removed, .. } = blobs::sweep(&mut self.conn, &self.dir);
        Ok(removed)
    }

    /// The earliest `expires_at` of any clip (pinned included, D-025), so the capture actor
    /// can run retention when it falls due. Works on read-only stores.
    pub fn next_expiry(&self) -> Result<Option<Timestamp>, StorageError> {
        let t: Option<i64> = self.conn.query_row(
            "SELECT MIN(expires_at) FROM clips WHERE expires_at IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(t.map(Timestamp))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::{NewClip, OpenMode, Storage, UpsertOutcome};
    use darkclip_core::{
        canonical_hash, classify, ClipId, Clock, Mime, RawRepresentation, Sensitivity, Timestamp,
    };
    use std::path::Path;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    pub const T0: i64 = 1_795_320_000_000;

    /// A clock the test sets explicitly.
    pub struct SetClock(pub AtomicI64);
    impl Clock for SetClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.load(Ordering::SeqCst))
        }
    }

    pub fn clock() -> Arc<SetClock> {
        Arc::new(SetClock(AtomicI64::new(T0)))
    }

    pub fn set(c: &Arc<SetClock>, t: i64) {
        c.0.store(t, Ordering::SeqCst);
    }

    pub fn open(dir: &Path, c: Arc<SetClock>) -> Storage {
        Storage::open(dir, OpenMode::ReadWrite, c).unwrap()
    }

    pub fn raw(mime: Mime, bytes: &[u8]) -> RawRepresentation {
        RawRepresentation {
            mime,
            platform_format: "test-format".into(),
            bytes: bytes.to_vec(),
            width: None,
            height: None,
            truncated: false,
        }
    }

    pub fn new_clip(reps: Vec<RawRepresentation>) -> NewClip {
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        NewClip {
            classified,
            hash,
            reps,
            source: None,
            sensitivity: Sensitivity::default(),
            expires_at: None,
            thumbnail_png: None,
            no_export: false,
        }
    }

    pub fn text_clip(s: &str) -> NewClip {
        new_clip(vec![raw(Mime::TextPlain, s.as_bytes())])
    }

    pub fn ins(s: &mut Storage, n: NewClip) -> ClipId {
        match s.upsert_clip(&n).unwrap() {
            UpsertOutcome::Inserted(id) => id,
            UpsertOutcome::Bumped(_) => panic!("expected Inserted"),
        }
    }

    pub fn png_bytes(seed: u8) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[seed, 2, 3, 255]).unwrap();
        }
        out
    }

    pub fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return out;
        };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    pub fn count(s: &Storage, sql: &str) -> i64 {
        s.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use crate::StorageError;
    use darkclip_core::{
        Mime, RetentionPolicy, SensitiveCategory, Sensitivity, SensitivityLevel, Timestamp,
    };

    const DAY: i64 = 86_400_000;

    fn no_limits() -> RetentionPolicy {
        RetentionPolicy {
            max_items: 0,
            max_age_days: 0,
            max_storage_mb: 0,
            ..RetentionPolicy::default()
        }
    }

    fn big(tag: u8) -> Vec<u8> {
        vec![tag; 600 * 1024]
    }

    fn insert_fts(s: &crate::Storage, id: &darkclip_core::ClipId) {
        let rowid: i64 = s
            .conn
            .query_row(
                "SELECT rowid FROM clip_rowid WHERE clip_id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO clips_fts (rowid, preview, body, note, tags) \
                 VALUES (?1, 'findable', 'findable', '', '')",
                [rowid],
            )
            .unwrap();
    }

    #[test]
    fn delete_removes_row_fts_and_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(b'a'))]));
        insert_fts(&s, &id);
        let hits = "SELECT count(*) FROM clips_fts WHERE clips_fts MATCH 'findable'";
        assert_eq!(count(&s, hits), 1);
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        s.delete_clip(&id).unwrap();
        assert!(s.get_clip(&id).unwrap().is_none());
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        assert_eq!(count(&s, "SELECT count(*) FROM representations"), 0);
        assert_eq!(count(&s, "SELECT count(*) FROM clip_rowid"), 0);
        assert_eq!(count(&s, hits), 0, "FTS row is gone");
        assert!(files_under(&tmp.path().join("blobs")).is_empty());
        assert_eq!(s.delete_clip(&id), Err(StorageError::NotFound));
    }

    #[test]
    fn delete_keeps_shared_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let png = png_bytes(9);
        let mut img = raw(Mime::ImagePng, &png);
        img.width = Some(1);
        img.height = Some(1);
        let a = ins(
            &mut s,
            new_clip(vec![raw(Mime::TextPlain, b"caption one"), img.clone()]),
        );
        let b = ins(
            &mut s,
            new_clip(vec![raw(Mime::TextPlain, b"caption two"), img]),
        );
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        s.delete_clip(&a).unwrap();
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        assert_eq!(s.read_representation(&b, 1).unwrap(), png);
        s.delete_clip(&b).unwrap();
        assert!(files_under(&tmp.path().join("blobs")).is_empty());
    }

    #[test]
    fn retention_count_keeps_pinned() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let mut ids = Vec::new();
        for i in 0..1005i64 {
            set(&c, T0 + i * 1000);
            ids.push(ins(&mut s, text_clip(&format!("clip number {i}"))));
        }
        // pin 10 of the oldest 20 (they would be evicted first otherwise)
        for id in ids.iter().step_by(2).take(10) {
            s.set_pinned(id, true).unwrap();
        }
        set(&c, T0 + 2_000_000);
        let p = RetentionPolicy {
            max_items: 1000,
            max_age_days: 0,
            max_storage_mb: 0,
            ..RetentionPolicy::default()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_count, 5);
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 1000);
        assert_eq!(count(&s, "SELECT count(*) FROM clips WHERE pinned = 1"), 10);
        // the 5 oldest unpinned (indexes 1,3,5,7,9) are gone
        for i in [1usize, 3, 5, 7, 9] {
            assert!(s.get_clip(&ids[i]).unwrap().is_none(), "{i}");
        }
        assert!(s.get_clip(&ids[11]).unwrap().is_some());
        assert!(r.bytes_freed > 0);
    }

    #[test]
    fn retention_count_pinned_exceed_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        for i in 0..10 {
            let id = ins(&mut s, text_clip(&format!("pinned {i}")));
            s.set_pinned(&id, true).unwrap();
        }
        for i in 0..3 {
            ins(&mut s, text_clip(&format!("loose {i}")));
        }
        let p = RetentionPolicy {
            max_items: 5,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_count, 3);
        assert_eq!(count(&s, "SELECT count(*) FROM clips WHERE pinned = 1"), 10);
        assert_eq!(count(&s, "SELECT count(*) FROM clips WHERE pinned = 0"), 0);
    }

    #[test]
    fn retention_age() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        set(&c, T0);
        let old = ins(&mut s, text_clip("old"));
        let old_pinned = ins(&mut s, text_clip("old pinned"));
        s.set_pinned(&old_pinned, true).unwrap();
        set(&c, T0 + 20 * DAY);
        let fresh = ins(&mut s, text_clip("fresh"));
        set(&c, T0 + 31 * DAY);
        let p = RetentionPolicy {
            max_age_days: 30,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_age, 1);
        assert!(s.get_clip(&old).unwrap().is_none());
        assert!(s.get_clip(&old_pinned).unwrap().is_some());
        assert!(s.get_clip(&fresh).unwrap().is_some());
    }

    #[test]
    fn retention_size() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let mut ids = Vec::new();
        for i in 0..3u8 {
            set(&c, T0 + i64::from(i) * 1000);
            ids.push(ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(i))])));
        }
        let pinned = ins(&mut s, text_clip("pinned small"));
        s.set_pinned(&pinned, true).unwrap();
        let before = s.stats().unwrap();
        assert!(before.blob_bytes >= 3 * 600 * 1024);
        let p = RetentionPolicy {
            max_storage_mb: 1,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_size, 2, "blob bytes count toward size");
        assert!(s.get_clip(&ids[0]).unwrap().is_none());
        assert!(s.get_clip(&ids[1]).unwrap().is_none());
        assert!(s.get_clip(&ids[2]).unwrap().is_some());
        assert!(s.get_clip(&pinned).unwrap().is_some());
        let after = s.stats().unwrap();
        assert!(after.db_bytes + after.blob_bytes <= 1024 * 1024);
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        assert!(r.bytes_freed >= 2 * 600 * 1024);
    }

    #[test]
    fn expired_masked_removed_first() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        set(&c, T0);
        let mut masked = text_clip("masked secret");
        masked.sensitivity = Sensitivity {
            level: SensitivityLevel::Masked,
            categories: vec![SensitiveCategory::ApiKey],
        };
        masked.expires_at = Some(Timestamp(T0 + 120_000));
        let secret = ins(&mut s, masked);
        let stored = s.get_clip(&secret).unwrap().unwrap();
        assert_eq!(stored.sensitivity.level, SensitivityLevel::Masked);
        assert_eq!(stored.validate(), Ok(()));
        let mut later = text_clip("not yet expired");
        later.expires_at = Some(Timestamp(T0 + 10 * DAY));
        let later = ins(&mut s, later);
        let plain = ins(&mut s, text_clip("plain"));
        set(&c, T0 + 120_000);
        let p = RetentionPolicy {
            max_items: 2,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.expired, 1, "expires_at <= now is removed first");
        assert_eq!(
            r.evicted_count, 0,
            "after expiry the count is within the limit"
        );
        assert!(s.get_clip(&secret).unwrap().is_none());
        assert!(s.get_clip(&later).unwrap().is_some());
        assert!(s.get_clip(&plain).unwrap().is_some());
    }

    #[test]
    fn expired_pinned_still_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let mut n = text_clip("pinned but expiring");
        n.expires_at = Some(Timestamp(T0 + 1000));
        let id = ins(&mut s, n);
        s.set_pinned(&id, true).unwrap();
        set(&c, T0 + 1000);
        let r = s.apply_retention(&no_limits()).unwrap();
        assert_eq!(r.expired, 1);
        assert!(s.get_clip(&id).unwrap().is_none());
    }

    #[test]
    fn retention_keeps_favorites() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let fav = s.favorites_id().unwrap();
        let mut ids = Vec::new();
        for i in 0..6i64 {
            set(&c, T0 + i * 1000);
            ids.push(ins(&mut s, text_clip(&format!("clip number {i}"))));
        }
        // The two oldest are favorites: count eviction skips them.
        s.add_to_collection(&fav, &ids[0]).unwrap();
        s.add_to_collection(&fav, &ids[1]).unwrap();
        // A user collection gives no protection.
        let col = s
            .create_collection("Work", darkclip_core::CollectionSort::Recent, None)
            .unwrap()
            .id;
        s.add_to_collection(&col, &ids[2]).unwrap();
        let p = RetentionPolicy {
            max_items: 4,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_count, 2);
        for (i, kept) in [true, true, false, false, true, true].iter().enumerate() {
            assert_eq!(s.get_clip(&ids[i]).unwrap().is_some(), *kept, "{i}");
        }
        // Age: an old favorite stays, an old non-favorite goes.
        set(&c, T0 + 40 * DAY);
        let p = RetentionPolicy {
            max_age_days: 30,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_age, 2);
        assert!(s.get_clip(&ids[0]).unwrap().is_some());
        assert!(s.get_clip(&ids[1]).unwrap().is_some());
        assert!(s.get_clip(&ids[4]).unwrap().is_none());
        // Size: favorites are never the victims.
        let large = vec![3u8; 1100 * 1024];
        let big_id = ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &large)]));
        s.add_to_collection(&fav, &big_id).unwrap();
        let p = RetentionPolicy {
            max_storage_mb: 1,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_size, 0);
        assert!(s.get_clip(&big_id).unwrap().is_some());
        // keep_favorites = false: favorites are evicted like any other clip.
        let p = RetentionPolicy {
            max_items: 1,
            keep_favorites: false,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_count, 2);
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 1);
    }

    #[test]
    fn expiry_beats_favorite() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let fav = s.favorites_id().unwrap();
        let mut n = text_clip("favorite but expiring");
        n.expires_at = Some(Timestamp(T0 + 1000));
        let id = ins(&mut s, n);
        s.add_to_collection(&fav, &id).unwrap();
        s.set_pinned(&id, true).unwrap();
        set(&c, T0 + 1000);
        let r = s.apply_retention(&RetentionPolicy::default()).unwrap();
        assert_eq!(r.expired, 1);
        assert!(s.get_clip(&id).unwrap().is_none());
        assert_eq!(count(&s, "SELECT count(*) FROM collection_clips"), 0);
        let favs = s.list_collections().unwrap();
        assert!(favs.iter().any(|c| c.id == fav && c.clip_count == 0));
    }

    #[test]
    fn next_expiry_is_the_earliest() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        assert_eq!(s.next_expiry(), Ok(None));
        ins(&mut s, text_clip("never expires"));
        assert_eq!(s.next_expiry(), Ok(None));
        let mut late = text_clip("late");
        late.expires_at = Some(Timestamp(T0 + 5_000));
        ins(&mut s, late);
        let mut early = text_clip("early");
        early.expires_at = Some(Timestamp(T0 + 1_000));
        ins(&mut s, early);
        assert_eq!(s.next_expiry(), Ok(Some(Timestamp(T0 + 1_000))));
        set(&c, T0 + 1_000);
        s.apply_retention(&no_limits()).unwrap();
        assert_eq!(s.next_expiry(), Ok(Some(Timestamp(T0 + 5_000))));
        // Read-only stores answer too (no write needed).
        let ro = crate::Storage::open(tmp.path(), crate::OpenMode::ReadOnly, c).unwrap();
        assert_eq!(ro.next_expiry(), Ok(Some(Timestamp(T0 + 5_000))));
    }

    #[test]
    fn orphan_and_stray_files_do_not_trigger_eviction() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let a = ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(1))]));
        let b = ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(2))]));
        let shard = tmp.path().join("blobs").join("ab");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join("e".repeat(64)), vec![0u8; 3 * 1024 * 1024]).unwrap();
        std::fs::write(
            tmp.path().join("blobs").join("Thumbs.db"),
            vec![0u8; 1024 * 1024],
        )
        .unwrap();
        let st = s.stats().unwrap();
        assert!(st.blob_bytes < 1024 * 1024 + 200 * 1024, "referenced only");
        let p = RetentionPolicy {
            max_storage_mb: 2,
            ..no_limits()
        };
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_size, 0);
        assert!(s.get_clip(&a).unwrap().is_some());
        assert!(s.get_clip(&b).unwrap().is_some());
    }

    #[test]
    fn clear_history_keep_pinned_true_and_false() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let a = ins(&mut s, text_clip("a"));
        ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(1))]));
        ins(&mut s, text_clip("c"));
        s.set_pinned(&a, true).unwrap();
        assert_eq!(s.clear_history(true).unwrap(), 2);
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 1);
        assert!(s.get_clip(&a).unwrap().is_some());
        assert!(files_under(&tmp.path().join("blobs")).is_empty());
        assert_eq!(s.clear_history(false).unwrap(), 1);
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        assert_eq!(count(&s, "SELECT count(*) FROM clip_rowid"), 0);
        assert_eq!(s.clear_history(false).unwrap(), 0);
    }

    #[test]
    fn sweep_orphans_after_simulated_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, new_clip(vec![raw(Mime::TextPlain, &big(7))]));
        let shard = tmp.path().join("blobs").join("ab");
        std::fs::create_dir_all(&shard).unwrap();
        let orphan = "c".repeat(64);
        std::fs::write(shard.join(&orphan), b"orphan").unwrap();
        std::fs::write(shard.join(format!("{}.tmp", "d".repeat(64))), b"partial").unwrap();
        std::fs::write(shard.join("notes.txt"), b"not a blob").unwrap();
        assert_eq!(s.sweep_orphan_blobs().unwrap(), 2);
        assert!(!shard.join(&orphan).exists());
        assert!(shard.join("notes.txt").exists(), "foreign files untouched");
        assert_eq!(s.read_representation(&id, 0).unwrap(), big(7));
        assert_eq!(s.sweep_orphan_blobs().unwrap(), 0);
    }

    #[test]
    fn read_only_store_refuses_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, text_clip("x"));
        drop(s);
        let mut ro = crate::Storage::open(tmp.path(), crate::OpenMode::ReadOnly, clock()).unwrap();
        assert_eq!(ro.delete_clip(&id), Err(StorageError::ReadOnly));
        assert_eq!(ro.clear_history(false), Err(StorageError::ReadOnly));
        assert_eq!(ro.sweep_orphan_blobs(), Err(StorageError::ReadOnly));
        assert_eq!(ro.stats().unwrap().clips, 1);
    }
}
