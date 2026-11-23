//! Storage statistics (`Storage::stats`).
//!
//! Measurements (also used by size-based retention):
//! - `db_bytes` = `(page_count - freelist_count) * page_size` of the main database:
//!   the pages in use. Free pages are not counted, so deleting rows lowers it
//!   immediately, without a `VACUUM`. The `-wal` file is not counted.
//! - `blob_bytes` = sum of the sizes of the blob files that rows reference (thumbnails
//!   included), each file once however many clips share it. Orphans and stray files
//!   are excluded, so they can never trigger eviction.

use crate::blobs;
use crate::{Storage, StorageError};
use darkclip_core::ClipType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageStats {
    pub clips: u64,
    pub pinned: u64,
    /// Members of the system Favorites collection.
    pub favorites: u64,
    /// Clip count per kind, ordered by the kind's serde name.
    pub by_kind: Vec<(ClipType, u64)>,
    pub db_bytes: u64,
    pub blob_bytes: u64,
}

fn count(n: i64) -> Result<u64, StorageError> {
    u64::try_from(n).map_err(|_| StorageError::Sqlite("ColumnDecode".into()))
}

impl Storage {
    pub(crate) fn db_bytes(&self) -> Result<u64, StorageError> {
        let pages: i64 = self
            .conn
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free: i64 = self
            .conn
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let size: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let used = pages.saturating_sub(free).max(0);
        Ok(count(used)?.saturating_mul(count(size)?))
    }

    /// Bytes of blob files referenced by rows (orphans and stray files excluded).
    pub(crate) fn blob_bytes(&self) -> Result<u64, StorageError> {
        blobs::referenced_bytes(&self.conn, &self.dir)
    }

    pub fn stats(&self) -> Result<StorageStats, StorageError> {
        let clips: i64 = self
            .conn
            .query_row("SELECT count(*) FROM clips", [], |r| r.get(0))?;
        let pinned: i64 =
            self.conn
                .query_row("SELECT count(*) FROM clips WHERE pinned = 1", [], |r| {
                    r.get(0)
                })?;
        let mut st = self
            .conn
            .prepare("SELECT kind, count(*) FROM clips GROUP BY kind ORDER BY kind")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut by_kind = Vec::new();
        for row in rows {
            let (kind, n) = row?;
            let kind: ClipType = serde_json::from_value(serde_json::Value::String(kind))
                .map_err(|_| StorageError::Sqlite("ColumnDecode".into()))?;
            by_kind.push((kind, count(n)?));
        }
        Ok(StorageStats {
            clips: count(clips)?,
            pinned: count(pinned)?,
            favorites: 0,
            by_kind,
            db_bytes: self.db_bytes()?,
            blob_bytes: self.blob_bytes()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::retention::test_support::*;
    use darkclip_core::{ClipType, Mime};

    #[test]
    fn stats_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let a = ins(&mut s, text_clip("alpha"));
        ins(&mut s, text_clip("beta"));
        ins(
            &mut s,
            new_clip(vec![raw(Mime::TextPlain, &vec![b'z'; 70 * 1024])]),
        );
        s.set_pinned(&a, true).unwrap();
        let st = s.stats().unwrap();
        assert_eq!(st.clips, 3);
        assert_eq!(st.pinned, 1);
        assert_eq!(st.by_kind, vec![(ClipType::Text, 3)]);
        assert!(st.db_bytes > 0);
        assert_eq!(st.blob_bytes, 70 * 1024);
    }
}
