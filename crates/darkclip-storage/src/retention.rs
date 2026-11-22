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
//!   Expired clips (`expires_at <= now`) are deleted even when pinned: the sensitive
//!   TTL is a privacy promise. Favorites do not exist yet (P1) and count as none.
//!   A limit of 0 (`max_items`, `max_age_days`, `max_storage_mb`) means no limit.
//! - Size = `Storage::db_bytes` + `Storage::blob_bytes` (see `stats.rs`). Victims are
//!   the oldest unpinned clips by `last_copied_at`; the amount freed is estimated
//!   from `size_bytes`, then re-measured, and the loop repeats until under the limit
//!   or only pinned clips remain.
//! - `bytes_freed` is the sum of the deleted clips' `size_bytes` (logical bytes; a
//!   blob shared with a surviving clip is counted though its file stays).

use crate::blobs::{self, Cleanup};
use crate::{ClipId, Storage, StorageError};
use darkclip_core::RetentionPolicy;
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
    out.shas.sort_unstable();
    out.shas.dedup();
    Ok(out)
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

fn checkpoint(conn: &Connection) {
    // Best-effort: a busy reader only delays the truncate.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

impl Storage {
    /// Post-commit cleanup shared by every deletion path.
    fn finish_delete(&mut self, d: &Deleted, force_checkpoint: bool) {
        if !d.shas.is_empty() {
            let _ = blobs::remove_unreferenced(&mut self.conn, &self.dir, &d.shas);
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
        self.finish_delete(&d, false);
        Ok(())
    }

    /// Deletes every clip (or every unpinned clip). Returns the number deleted.
    pub fn clear_history(&mut self, keep_pinned: bool) -> Result<u64, StorageError> {
        self.ensure_writable()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = if keep_pinned {
            "SELECT id FROM clips WHERE pinned = 0"
        } else {
            "SELECT id FROM clips"
        };
        let ids = select_ids(&tx, sql, [])?;
        let d = delete_ids(&tx, &ids)?;
        tx.commit()?;
        self.finish_delete(&d, true);
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
                "SELECT id FROM clips WHERE pinned = 0 AND last_copied_at < ?1",
                [cutoff],
            )?;
            let d = delete_ids(&tx, &ids)?;
            report.evicted_age = d.count;
            all.add(d);
        }
        // 3. count (pinned count toward the total but are never evicted)
        if p.max_items > 0 {
            let total: i64 = tx.query_row("SELECT count(*) FROM clips", [], |r| r.get(0))?;
            let excess = to_u64(total).saturating_sub(u64::from(p.max_items));
            if excess > 0 {
                let ids = select_ids(
                    &tx,
                    "SELECT id FROM clips WHERE pinned = 0 \
                     ORDER BY last_copied_at ASC, created_at ASC, id ASC LIMIT ?1",
                    [i64::try_from(excess).unwrap_or(i64::MAX)],
                )?;
                let d = delete_ids(&tx, &ids)?;
                report.evicted_count = d.count;
                all.add(d);
            }
        }
        tx.commit()?;
        self.finish_delete(&all, false);
        let mut deleted_total = all.count;
        let mut freed = all.bytes;

        // 4. size
        if p.max_storage_mb > 0 {
            let limit = u64::from(p.max_storage_mb).saturating_mul(1024 * 1024);
            loop {
                let total = self.db_bytes()?.saturating_add(self.blob_bytes());
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
                    let mut st = tx.prepare(
                        "SELECT id, size_bytes FROM clips WHERE pinned = 0 \
                         ORDER BY last_copied_at ASC, created_at ASC, id ASC",
                    )?;
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
                    break; // only pinned clips remain
                }
                let d = delete_ids(&tx, &ids)?;
                tx.commit()?;
                report.evicted_size += d.count;
                freed = freed.saturating_add(d.bytes);
                deleted_total += d.count;
                self.finish_delete(&d, false);
            }
        }
        report.bytes_freed = freed;
        if deleted_total > CHECKPOINT_THRESHOLD {
            checkpoint(&self.conn);
        }
        Ok(report)
    }

    /// Removes blob files (and `.tmp` leftovers) that nothing references. Returns the
    /// number removed.
    pub fn sweep_orphan_blobs(&mut self) -> Result<u64, StorageError> {
        self.ensure_writable()?;
        let Cleanup { removed, .. } = blobs::sweep(&mut self.conn, &self.dir);
        Ok(removed)
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
        }
    }

    pub fn text_clip(s: &str) -> NewClip {
        new_clip(vec![raw(Mime::TextPlain, s.as_bytes())])
    }

    pub fn ins(s: &mut Storage, n: NewClip) -> ClipId {
        match s.upsert_clip(n).unwrap() {
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
    use darkclip_core::{Mime, RetentionPolicy, Sensitivity, Timestamp};

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
        masked.expires_at = Some(Timestamp(T0 + 120_000));
        let secret = ins(&mut s, masked);
        let mut later = text_clip("not yet expired");
        later.expires_at = Some(Timestamp(T0 + 10 * DAY));
        let later = ins(&mut s, later);
        let plain = ins(&mut s, text_clip("plain"));
        let _ = Sensitivity::default();
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
