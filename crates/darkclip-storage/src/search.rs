//! Full-text search and listing (`docs/architecture/data-model.md` §4).
//!
//! Ranking: pinned first (`pin_order`), then bm25 for text queries, then
//! `last_copied_at DESC`. User text is never interpolated into MATCH syntax: every
//! token is quoted (`"tok"*`, internal quotes doubled). Query text is never logged and
//! never appears in an error.
//!
//! Pagination: keyset on `(last_copied_at, id)` over the unpinned rows of the recent
//! list (empty text). Pinned rows that match the filters are returned on the first page
//! only and do not count toward `limit`. Text queries are ranked by bm25, which a
//! `(last_copied_at, id)` cursor cannot resume, so they return a single page (at most
//! `limit` rows, never a cursor) and ignore `cursor`.
//!
//! Queries shorter than 3 characters, or without any letter or digit, cannot use the
//! prefix index usefully; they fall back to a case-insensitive substring match on the
//! `preview` of the most recent [`FALLBACK_WINDOW`] clips.

use crate::{Storage, StorageError};
use darkclip_core::{ClipId, ClipMetadata, ClipType, SensitivityLevel, Timestamp};
use rusqlite::{params_from_iter, types::Value, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Bytes of plain text indexed per clip.
pub(crate) const BODY_MAX_BYTES: usize = 32 * 1024;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;
/// Recent rows scanned by the substring fallback.
const FALLBACK_WINDOW: i64 = 2000;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub kinds: Vec<ClipType>,
    pub pinned_only: bool,
    pub since: Option<Timestamp>,
    pub include_masked: bool,
    pub limit: u16,
    pub cursor: Option<Cursor>,
}

/// Position of the last row of a page. Opaque outside this crate: use `encode`/`decode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub last_copied_at: Timestamp,
    pub id: ClipId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipSummary {
    pub id: ClipId,
    pub kind: ClipType,
    pub preview: String,
    pub pinned: bool,
    pub last_copied_at: Timestamp,
    pub sensitivity: SensitivityLevel,
    pub thumb: Option<[u8; 32]>,
}

impl Cursor {
    /// Opaque base64url (no padding) of 8 big-endian timestamp bytes + 16 id bytes.
    pub fn encode(&self) -> String {
        let mut raw = [0u8; 24];
        raw[..8].copy_from_slice(&self.last_copied_at.0.to_be_bytes());
        raw[8..].copy_from_slice(&id_bytes(&self.id));
        let mut out = String::with_capacity(32);
        for chunk in raw.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..=chunk.len() {
                let idx = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
                out.push(char::from(B64[idx]));
            }
        }
        out
    }

    /// Returns `None` for anything `encode` could not have produced.
    pub fn decode(s: &str) -> Option<Cursor> {
        if s.len() != 32 {
            return None;
        }
        let mut raw = Vec::with_capacity(24);
        for chunk in s.as_bytes().chunks(4) {
            let mut n = 0u32;
            for c in chunk {
                let v = B64.iter().position(|x| x == c)?;
                n = (n << 6) | u32::try_from(v).ok()?;
            }
            for i in 0..3 {
                raw.push(u8::try_from((n >> (16 - 8 * i)) & 0xff).ok()?);
            }
        }
        let ts = i64::from_be_bytes(raw.get(..8)?.try_into().ok()?);
        let id = id_from_bytes(raw.get(8..24)?)?;
        let cur = Cursor {
            last_copied_at: Timestamp(ts),
            id,
        };
        // Reject non-canonical encodings (nonzero trailing bits).
        (cur.encode() == s).then_some(cur)
    }
}

fn id_bytes(id: &ClipId) -> [u8; 16] {
    let hex: Vec<u8> = id.to_string().bytes().filter(|b| *b != b'-').collect();
    let mut out = [0u8; 16];
    for (slot, pair) in out.iter_mut().zip(hex.chunks(2)) {
        if let [hi, lo] = pair {
            let hi = char::from(*hi).to_digit(16).unwrap_or(0);
            let lo = char::from(*lo).to_digit(16).unwrap_or(0);
            *slot = u8::try_from(hi * 16 + lo).unwrap_or(0);
        }
    }
    out
}

fn id_from_bytes(b: &[u8]) -> Option<ClipId> {
    if b.len() != 16 {
        return None;
    }
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let s = format!(
        "{}-{}-{}-{}-{}",
        h.get(0..8)?,
        h.get(8..12)?,
        h.get(12..16)?,
        h.get(16..20)?,
        h.get(20..32)?
    );
    ClipId::parse(&s).ok()
}

fn enum_name<T: Serialize>(v: &T) -> Result<String, StorageError> {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(StorageError::Sqlite("ColumnEncode".into())),
    }
}

fn enum_parse<T: DeserializeOwned>(s: String) -> Result<T, StorageError> {
    serde_json::from_value(serde_json::Value::String(s))
        .map_err(|_| StorageError::Sqlite("ColumnDecode".into()))
}

/// The first [`BODY_MAX_BYTES`] bytes of `text`, cut at a char boundary.
pub(crate) fn truncate_body(text: &str) -> &str {
    if text.len() <= BODY_MAX_BYTES {
        return text;
    }
    let mut end = BODY_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Inserts the FTS row of a clip, replacing any previous one (delete, then insert).
/// Masked clips pass an empty `body`/`note`: only the masked preview is indexed.
pub(crate) fn index_clip(
    conn: &Connection,
    clip_id: &str,
    preview: &str,
    body: &str,
    note: &str,
) -> Result<(), StorageError> {
    let rowid: i64 = conn.query_row(
        "SELECT rowid FROM clip_rowid WHERE clip_id = ?1",
        [clip_id],
        |r| r.get(0),
    )?;
    conn.execute("DELETE FROM clips_fts WHERE rowid = ?1", [rowid])?;
    conn.execute(
        "INSERT INTO clips_fts (rowid, preview, body, note, tags) VALUES (?1, ?2, ?3, ?4, '')",
        rusqlite::params![rowid, preview, body, note],
    )?;
    Ok(())
}

/// `"tok"* "tok2"*` from user text; `None` when no token has a letter or digit.
fn match_expr(text: &str) -> Option<String> {
    let toks: Vec<String> = text
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    (!toks.is_empty()).then(|| toks.join(" "))
}

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

const COLS: &str =
    "c.id, c.kind, c.preview, c.pinned, c.last_copied_at, c.sensitivity_level, c.meta";

fn row_to_summary(r: &rusqlite::Row<'_>) -> Result<ClipSummary, StorageError> {
    let decode = || StorageError::Sqlite("ColumnDecode".into());
    let id: String = r.get(0)?;
    let kind: String = r.get(1)?;
    let level: String = r.get(5)?;
    let meta: String = r.get(6)?;
    let meta: ClipMetadata = serde_json::from_str(&meta).map_err(|_| decode())?;
    Ok(ClipSummary {
        id: ClipId::parse(&id).map_err(|_| decode())?,
        kind: enum_parse(kind)?,
        preview: r.get(2)?,
        pinned: r.get(3)?,
        last_copied_at: Timestamp(r.get(4)?),
        sensitivity: enum_parse(level)?,
        thumb: meta.image_thumb_sha256,
    })
}

impl Storage {
    /// Searches clips. See the module docs for ranking and pagination.
    pub fn search(
        &self,
        q: &SearchQuery,
    ) -> Result<(Vec<ClipSummary>, Option<Cursor>), StorageError> {
        let limit = match usize::from(q.limit) {
            0 => DEFAULT_LIMIT,
            n => n.min(MAX_LIMIT),
        };
        let text = q.text.as_deref().map(str::trim).filter(|t| !t.is_empty());

        // Shared filters; every value is a bound parameter.
        let mut filt = String::new();
        let mut args: Vec<Value> = Vec::new();
        if !q.kinds.is_empty() {
            let mut marks = Vec::new();
            for k in &q.kinds {
                args.push(Value::Text(enum_name(k)?));
                marks.push("?");
            }
            filt.push_str(&format!(" AND c.kind IN ({})", marks.join(",")));
        }
        if q.pinned_only {
            filt.push_str(" AND c.pinned = 1");
        }
        if let Some(since) = q.since {
            args.push(Value::Integer(since.0));
            filt.push_str(" AND c.last_copied_at >= ?");
        }
        if !q.include_masked {
            filt.push_str(" AND c.sensitivity_level <> 'masked'");
        }

        let Some(text) = text else {
            return self.recent(&filt, args, limit, q);
        };
        let fts = match_expr(text).filter(|_| text.chars().count() >= 3);
        let rows = if let Some(expr) = fts {
            // The MATCH placeholder comes first in the statement, so its value does too.
            args.insert(0, Value::Text(expr));
            let sql = format!(
                "SELECT {COLS} FROM clips_fts f \
                 JOIN clip_rowid r ON r.rowid = f.rowid \
                 JOIN clips c ON c.id = r.clip_id \
                 WHERE clips_fts MATCH ? {filt} \
                 ORDER BY c.pinned DESC, c.pin_order ASC, \
                 bm25(clips_fts, 4.0, 1.0, 1.0, 1.0), c.last_copied_at DESC, c.id DESC \
                 LIMIT {limit}"
            );
            self.collect(&sql, args)?
        } else {
            // The window subquery has no placeholders, so filter values stay in order.
            args.push(Value::Text(format!("%{}%", like_escape(text))));
            let sql = format!(
                "SELECT {COLS} FROM (SELECT * FROM clips \
                 ORDER BY last_copied_at DESC, id DESC LIMIT {FALLBACK_WINDOW}) c \
                 WHERE 1 = 1 {filt} AND c.preview LIKE ? ESCAPE '\\' \
                 ORDER BY c.pinned DESC, c.pin_order ASC, c.last_copied_at DESC, c.id DESC \
                 LIMIT {limit}"
            );
            self.collect(&sql, args)?
        };
        Ok((rows, None))
    }

    fn collect(&self, sql: &str, args: Vec<Value>) -> Result<Vec<ClipSummary>, StorageError> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(row_to_summary(r)?);
        }
        Ok(out)
    }

    /// Recent list: pinned rows (first page only), then unpinned by keyset.
    fn recent(
        &self,
        filt: &str,
        args: Vec<Value>,
        limit: usize,
        q: &SearchQuery,
    ) -> Result<(Vec<ClipSummary>, Option<Cursor>), StorageError> {
        let mut out = Vec::new();
        if q.cursor.is_none() {
            let sql = format!(
                "SELECT {COLS} FROM clips c WHERE c.pinned = 1 {filt} \
                 ORDER BY c.pin_order ASC, c.last_copied_at DESC, c.id DESC"
            );
            out = self.collect(&sql, args.clone())?;
        }
        if q.pinned_only {
            return Ok((out, None));
        }
        let mut a = args;
        let mut keyset = "";
        if let Some(c) = q.cursor {
            a.push(Value::Integer(c.last_copied_at.0));
            a.push(Value::Integer(c.last_copied_at.0));
            a.push(Value::Text(c.id.to_string()));
            keyset = " AND (c.last_copied_at < ? OR (c.last_copied_at = ? AND c.id < ?))";
        }
        let sql = format!(
            "SELECT {COLS} FROM clips c WHERE c.pinned = 0 {filt} {keyset} \
             ORDER BY c.last_copied_at DESC, c.id DESC LIMIT {}",
            limit + 1
        );
        let mut page = self.collect(&sql, a)?;
        let next = if page.len() > limit {
            page.truncate(limit);
            page.last().map(|s| Cursor {
                last_copied_at: s.last_copied_at,
                id: s.id,
            })
        } else {
            None
        };
        out.extend(page);
        Ok((out, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewClip, OpenMode, UpsertOutcome};
    use darkclip_core::{
        canonical_hash, classify, ClipboardSource, Clock, Mime, Platform, RawRepresentation,
        SensitiveCategory, Sensitivity, SourceConfidence,
    };
    use std::path::Path;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    struct TestClock(AtomicI64);
    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.fetch_add(1000, Ordering::SeqCst))
        }
    }

    fn open(dir: &Path) -> Storage {
        let clock: Arc<dyn Clock> = Arc::new(TestClock(AtomicI64::new(1_795_320_000_000)));
        Storage::open(dir, OpenMode::ReadWrite, clock).unwrap()
    }

    fn raw(mime: Mime, bytes: &[u8]) -> RawRepresentation {
        RawRepresentation {
            mime,
            platform_format: "test-format".into(),
            bytes: bytes.to_vec(),
            width: None,
            height: None,
            truncated: false,
        }
    }

    fn text_clip(s: &str) -> NewClip {
        let reps = vec![raw(Mime::TextPlain, s.as_bytes())];
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        NewClip {
            classified,
            hash,
            reps,
            source: Some(ClipboardSource {
                app_id: "org.example.editor".into(),
                app_name: None,
                platform: Platform::Windows,
                confidence: SourceConfidence::Owner,
            }),
            sensitivity: Sensitivity::default(),
            expires_at: None,
            thumbnail_png: None,
        }
    }

    /// A masked clip: storage is given the masked preview, never the secret.
    fn masked_clip(secret: &str, masked_preview: &str) -> NewClip {
        let mut c = text_clip(secret);
        c.classified.preview = masked_preview.into();
        c.expires_at = Some(Timestamp(1_795_320_000_000 + 3_600_000));
        c.sensitivity = Sensitivity {
            level: SensitivityLevel::Masked,
            categories: vec![SensitiveCategory::ApiKey],
        };
        c
    }

    fn id_of(o: UpsertOutcome) -> ClipId {
        match o {
            UpsertOutcome::Inserted(i) | UpsertOutcome::Bumped(i) => i,
        }
    }

    fn q(text: &str) -> SearchQuery {
        SearchQuery {
            text: Some(text.into()),
            kinds: vec![],
            pinned_only: false,
            since: None,
            include_masked: true,
            limit: 50,
            cursor: None,
        }
    }

    fn previews(r: &(Vec<ClipSummary>, Option<Cursor>)) -> Vec<&str> {
        r.0.iter().map(|s| s.preview.as_str()).collect()
    }

    #[test]
    fn search_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(text_clip("kubectl get pods")).unwrap();
        s.upsert_clip(text_clip("unrelated words")).unwrap();
        let r = s.search(&q("kube")).unwrap();
        assert_eq!(previews(&r), vec!["kubectl get pods"]);
    }

    #[test]
    fn search_matches_body_beyond_preview() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let long = format!("{} needlebody", "filler ".repeat(200));
        s.upsert_clip(text_clip(&long)).unwrap();
        assert_eq!(s.search(&q("needlebody")).unwrap().0.len(), 1);
    }

    #[test]
    fn search_diacritic_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(text_clip("Tiếng Việt")).unwrap();
        let r = s.search(&q("tieng viet")).unwrap();
        assert_eq!(previews(&r), vec!["Tiếng Việt"]);
    }

    #[test]
    fn search_short_query_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(text_clip("mydb_backup")).unwrap();
        s.upsert_clip(text_clip("nothing here")).unwrap();
        let r = s.search(&q("db")).unwrap();
        assert_eq!(previews(&r), vec!["mydb_backup"]);
    }

    #[test]
    fn search_operators_are_literal() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(text_clip("alpha beta")).unwrap();
        s.upsert_clip(text_clip("NOT OR gate")).unwrap();
        s.upsert_clip(text_clip("say \"hi\" to NOT")).unwrap();
        // Operators are plain words: "NOT OR" needs both words as prefixes.
        let r = s.search(&q("NOT OR \"")).unwrap();
        assert_eq!(previews(&r), vec!["NOT OR gate"]);
        // Other FTS syntax is inert too.
        for t in [
            "alpha*",
            "a AND b",
            "alpha NEAR beta",
            "col:alpha",
            "(alpha",
            "\"",
            "-",
        ] {
            s.search(&q(t)).unwrap();
        }
        assert_eq!(s.search(&q("alpha OR zzz")).unwrap().0.len(), 0);
    }

    #[test]
    fn masked_excluded_from_mcp_style_query() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(masked_clip("sk_live_hunter2secret", "sk_l…[masked]"))
            .unwrap();
        s.upsert_clip(text_clip("a normal masked-looking note"))
            .unwrap();
        let mut mcp = q("masked");
        mcp.include_masked = false;
        let r = s.search(&mcp).unwrap();
        assert_eq!(previews(&r), vec!["a normal masked-looking note"]);
        // Included when asked; matched through the masked preview only.
        assert_eq!(s.search(&q("masked")).unwrap().0.len(), 2);
        // Recent list honours the flag as well.
        let mut list = q("");
        list.text = None;
        list.include_masked = false;
        assert_eq!(s.search(&list).unwrap().0.len(), 1);
    }

    #[test]
    fn masked_body_is_never_indexed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(masked_clip("hunter2secretvalue", "hunt…[masked]"))
            .unwrap();
        assert!(s.search(&q("hunter2secretvalue")).unwrap().0.is_empty());
        let n: i64 = s
            .conn
            .query_row(
                "SELECT count(*) FROM clips_fts WHERE clips_fts MATCH 'hunter2secretvalue'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn bump_replaces_fts_row() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let id = id_of(s.upsert_clip(text_clip("kubectl get pods")).unwrap());
        let again = s.upsert_clip(text_clip("kubectl get pods")).unwrap();
        assert_eq!(again, UpsertOutcome::Bumped(id));
        let n: i64 = s
            .conn
            .query_row("SELECT count(*) FROM clips_fts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.search(&q("kube")).unwrap().0.len(), 1);
    }

    #[test]
    fn body_is_limited_to_32_kib() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let text = format!("{} tailmarker", "a ".repeat(20_000));
        s.upsert_clip(text_clip(&text)).unwrap();
        assert!(s.search(&q("tailmarker")).unwrap().0.is_empty());
        assert_eq!(truncate_body("é".repeat(40_000).as_str()).len(), 32 * 1024);
    }

    #[test]
    fn cursor_roundtrip_and_rejects_garbage() {
        let c = Cursor {
            last_copied_at: Timestamp(1_795_320_123_456),
            id: ClipId::new_v7(Timestamp(1_795_320_123_456)),
        };
        let s = c.encode();
        assert_eq!(s.len(), 32);
        assert!(s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(Cursor::decode(&s), Some(c));
        assert_eq!(Cursor::decode(""), None);
        assert_eq!(Cursor::decode("not a cursor"), None);
        assert_eq!(Cursor::decode(&"A".repeat(32)), None);
    }

    #[test]
    fn pagination_stable_under_insert() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        for i in 0..10 {
            s.upsert_clip(text_clip(&format!("item number {i}")))
                .unwrap();
        }
        let mut query = q("");
        query.text = None;
        query.limit = 4;
        let (p1, c1) = s.search(&query).unwrap();
        assert_eq!(p1.len(), 4);
        // A new clip arrives between pages.
        s.upsert_clip(text_clip("brand new arrival")).unwrap();
        query.cursor = Some(Cursor::decode(&c1.unwrap().encode()).unwrap());
        let (p2, c2) = s.search(&query).unwrap();
        query.cursor = Some(c2.unwrap());
        let (p3, c3) = s.search(&query).unwrap();
        assert!(c3.is_none());
        let all: Vec<String> = p1
            .iter()
            .chain(&p2)
            .chain(&p3)
            .map(|c| c.preview.clone())
            .collect();
        let expect: Vec<String> = (0..10).rev().map(|i| format!("item number {i}")).collect();
        assert_eq!(all, expect);
    }

    #[test]
    fn kinds_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.upsert_clip(text_clip("https://example.com/page"))
            .unwrap();
        s.upsert_clip(text_clip("just some text")).unwrap();
        let mut query = q("");
        query.text = None;
        query.kinds = vec![ClipType::Url];
        let r = s.search(&query).unwrap();
        assert_eq!(r.0.len(), 1);
        assert_eq!(r.0[0].kind, ClipType::Url);
        query.text = Some("example".into());
        assert_eq!(s.search(&query).unwrap().0.len(), 1);
        query.kinds = vec![ClipType::Image];
        assert!(s.search(&query).unwrap().0.is_empty());
    }

    #[test]
    fn pinned_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let old = id_of(s.upsert_clip(text_clip("pin target old")).unwrap());
        s.upsert_clip(text_clip("middle clip")).unwrap();
        s.upsert_clip(text_clip("newest clip")).unwrap();
        s.set_pinned(&old, true).unwrap();
        let mut query = q("");
        query.text = None;
        let r = s.search(&query).unwrap();
        assert_eq!(
            previews(&r),
            vec!["pin target old", "newest clip", "middle clip"]
        );
        assert!(r.0[0].pinned);
        // Also in text results: the pinned match outranks a newer one.
        let r = s.search(&q("clip")).unwrap();
        assert_eq!(r.0.len(), 2);
        let mut only = q("");
        only.text = None;
        only.pinned_only = true;
        assert_eq!(s.search(&only).unwrap().0.len(), 1);
        let mut since = q("");
        since.text = None;
        since.since = Some(Timestamp(i64::MAX));
        assert!(s.search(&since).unwrap().0.is_empty());
    }

    #[test]
    #[ignore = "benchmark; run in release with --ignored --nocapture"]
    fn bench_search() {
        let words = [
            "kubectl", "deploy", "backup", "server", "config", "token", "database", "request",
            "response", "docker", "compose", "branch", "commit", "release", "pipeline", "cache",
            "network", "socket", "thread", "memory", "buffer", "stream", "render", "layout",
            "invoice", "meeting", "report", "budget", "travel", "recipe",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        for i in 0..10_000u64 {
            let target = 50 + usize::try_from(next() % 1951).unwrap();
            let mut text = format!("clip{i} ");
            while text.len() < target {
                text.push_str(words[usize::try_from(next() % 30).unwrap()]);
                text.push(' ');
            }
            s.upsert_clip(text_clip(&text)).unwrap();
        }
        let mut times = Vec::new();
        for n in 0..200usize {
            let w = words[usize::try_from(next() % 30).unwrap()];
            let text = match n % 3 {
                0 => w[..4].to_string(),
                1 => format!("{} {}", &w[..5], words[n % 30]),
                _ => w.to_string(),
            };
            let query = SearchQuery {
                limit: 50,
                ..q(&text)
            };
            let t = std::time::Instant::now();
            let r = s.search(&query).unwrap();
            times.push(t.elapsed());
            assert!(!r.0.is_empty());
        }
        times.sort();
        let p50 = times[100];
        let p95 = times[189];
        println!("bench_search: p50={p50:?} p95={p95:?}");
        assert!(p95 <= std::time::Duration::from_millis(30), "p95 {p95:?}");
    }
}
