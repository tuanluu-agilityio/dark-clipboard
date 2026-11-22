//! `--seed-demo-clips N` (dev flag, Task 204): writes N synthetic text clips into `--data-dir`
//! so scrolling and search can be tried with a large history. Never the default data dir.
//!
//! The texts are made up from fixed word lists (English and Vietnamese) and an index; nothing
//! comes from the clipboard, and no text looks like a secret. Clips are spread over the last 30
//! days and three made-up source apps. Nothing here is logged.

use darkclip_core::{
    canonical_hash, classify, ClipboardSource, Clock, Mime, Platform, RawRepresentation,
    Sensitivity, SourceConfidence, Timestamp,
};
use darkclip_storage::{NewClip, OpenMode, Storage, StorageError};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// Largest N accepted.
pub const MAX_DEMO_CLIPS: u32 = 100_000;
/// The clips are spread over this many milliseconds before now.
const SPREAD_MS: i64 = 30 * 86_400_000;

/// True when seeding may write into `data_dir`: it must be given and must not be (or be inside)
/// the default data dir. Paths are compared after canonicalisation when they exist.
pub fn seed_target_allowed(data_dir: Option<&Path>, default: Option<&Path>) -> bool {
    let Some(dir) = data_dir else {
        return false;
    };
    let Some(default) = default else {
        return true;
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    // A folder that does not exist yet: canonicalise its nearest existing parent.
    let resolve = |p: &Path| {
        let mut tail = Vec::new();
        let mut cur = p.to_path_buf();
        loop {
            if cur.exists() {
                let mut out = canon(&cur);
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return out;
            }
            match (cur.file_name().map(|n| n.to_os_string()), cur.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name);
                    cur = parent.to_path_buf();
                }
                _ => return p.to_path_buf(),
            }
        }
    };
    let (dir, default) = (resolve(dir), resolve(default));
    !dir.starts_with(&default)
}

const EN: &[&str] = &[
    "meeting", "notes", "release", "draft", "invoice", "summary", "kubectl", "deploy", "review",
    "budget", "travel", "recipe", "project", "design", "backlog", "weekly", "report", "agenda",
];
const VI: &[&str] = &[
    "Việt Nam",
    "cà phê",
    "phở bò",
    "đồng hồ",
    "bảng nhớ tạm",
    "họp nhóm",
    "Đà Nẵng",
    "ghi chú",
    "tiếng Việt",
    "nhật ký",
    "hóa đơn",
    "kế hoạch",
    "báo cáo",
    "ủy ban",
];

fn pick(list: &[&'static str], n: u32) -> &'static str {
    let len = u32::try_from(list.len()).unwrap_or(1).max(1);
    let i = usize::try_from(n % len).unwrap_or(0);
    list.get(i).copied().unwrap_or("demo")
}

/// The synthetic text of clip `i`.
pub fn demo_text(i: u32) -> String {
    let (a, b, c) = (
        pick(EN, i),
        pick(VI, i / 3),
        pick(EN, i.wrapping_mul(7).wrapping_add(3)),
    );
    match i % 10 {
        3 => format!("https://example.com/demo/{a}/{i}"),
        7 => {
            format!("fn demo_{a}_{i}() {{\n    let total = {i};\n    println!(\"{{total}}\");\n}}")
        }
        _ => format!("Demo {i}: {a} {b} {c}"),
    }
}

/// When clip `i` of `n` was copied: newest first, spread over 30 days before `now`.
pub fn demo_time(i: u32, n: u32, now: Timestamp) -> Timestamp {
    let step = SPREAD_MS / i64::from(n.max(1));
    Timestamp(
        now.0
            .saturating_sub(i64::from(i).saturating_mul(step.max(1))),
    )
}

fn demo_source(i: u32) -> ClipboardSource {
    let (id, name) = match i % 3 {
        0 => ("demo-editor.exe", "Demo Editor"),
        1 => ("demo-browser.exe", "Demo Browser"),
        _ => ("demo-terminal.exe", "Demo Terminal"),
    };
    ClipboardSource {
        app_id: id.into(),
        app_name: Some(name.into()),
        platform: Platform::Windows,
        confidence: SourceConfidence::Owner,
    }
}

/// Writes `n` demo clips into the storage at `dir`; returns how many were stored. The oldest
/// clip is written first, so the storage's own order matches the made-up times.
pub fn seed(dir: &Path, n: u32, now: Timestamp) -> Result<u32, StorageError> {
    let clock = Arc::new(SeedClock(AtomicI64::new(now.0)));
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let mut storage = Storage::open(dir, OpenMode::ReadWrite, dyn_clock)?;
    let mut stored = 0u32;
    for i in (0..n.min(MAX_DEMO_CLIPS)).rev() {
        let text = demo_text(i);
        let reps = vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: text.into_bytes(),
            width: None,
            height: None,
            truncated: false,
        }];
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        let clip = NewClip {
            classified,
            hash,
            reps,
            source: Some(demo_source(i)),
            sensitivity: Sensitivity::default(),
            expires_at: None,
            thumbnail_png: None,
        };
        clock.0.store(demo_time(i, n, now).0, Ordering::SeqCst);
        storage.upsert_clip(&clip)?;
        stored = stored.saturating_add(1);
    }
    Ok(stored)
}

// Storage clock for seeding: the time of the clip being written.
struct SeedClock(AtomicI64);

impl Clock for SeedClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::ClipType;
    use darkclip_storage::SearchQuery;
    use std::collections::HashSet;

    const NOW: Timestamp = Timestamp(1_795_429_800_000);

    fn recent(limit: u16) -> SearchQuery {
        SearchQuery {
            text: None,
            kinds: Vec::new(),
            pinned_only: false,
            since: None,
            include_masked: true,
            limit,
            cursor: None,
        }
    }

    #[test]
    fn never_the_default_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let default = tmp.path().join("DarkClipboard");
        std::fs::create_dir_all(&default).unwrap();
        let other = tmp.path().join("demo");
        assert!(!seed_target_allowed(None, Some(&default)));
        assert!(!seed_target_allowed(Some(&default), Some(&default)));
        assert!(!seed_target_allowed(
            Some(&default.join("sub")),
            Some(&default)
        ));
        // The same folder written differently is still the default.
        let dotted = tmp.path().join("demo").join("..").join("DarkClipboard");
        std::fs::create_dir_all(tmp.path().join("demo")).unwrap();
        assert!(!seed_target_allowed(Some(&dotted), Some(&default)));
        assert!(seed_target_allowed(Some(&other), Some(&default)));
        // Without a default dir (no OS data folder) a given dir is fine.
        assert!(seed_target_allowed(Some(&other), None));
    }

    #[test]
    fn demo_texts_are_unique_varied_and_secret_free() {
        let texts: Vec<String> = (0..500).map(demo_text).collect();
        let unique: HashSet<&String> = texts.iter().collect();
        assert_eq!(unique.len(), texts.len());
        assert!(texts.iter().all(|t| !t.is_empty()));
        // Vietnamese words with diacritics are part of the mix (search and highlight demos).
        assert!(texts.iter().any(|t| t.contains("Việt")));
        // Classified kinds vary: plain text and links. Code-like snippets are in the mix too,
        // though the P0 classifier stores them as text (it has no code detection yet).
        let kinds: HashSet<ClipType> = texts
            .iter()
            .map(|t| {
                let reps = vec![RawRepresentation {
                    mime: Mime::TextPlain,
                    platform_format: "demo".into(),
                    bytes: t.as_bytes().to_vec(),
                    width: None,
                    height: None,
                    truncated: false,
                }];
                classify(&reps).kind
            })
            .collect();
        assert!(kinds.contains(&ClipType::Text));
        assert!(kinds.contains(&ClipType::Url));
        assert!(texts.iter().any(|t| t.starts_with("fn demo_")));
        // The core detector finds nothing sensitive in them (every category enabled).
        use darkclip_core::SensitiveCategory as C;
        let all = [
            C::ApiKey,
            C::Token,
            C::PrivateKey,
            C::Password,
            C::PaymentCard,
            C::Email,
            C::Phone,
        ];
        for (i, t) in texts.iter().enumerate() {
            assert!(
                darkclip_core::sensitive::scan(t, &all)
                    .categories
                    .is_empty(),
                "demo text {i} looks sensitive"
            );
        }
    }

    #[test]
    fn demo_times_go_back_thirty_days() {
        assert_eq!(demo_time(0, 1000, NOW), NOW);
        let last = demo_time(999, 1000, NOW);
        assert!(last.0 < NOW.0 && last.0 >= NOW.0 - SPREAD_MS);
        assert!(demo_time(10, 1000, NOW).0 > demo_time(11, 1000, NOW).0);
    }

    #[test]
    fn seed_writes_n_searchable_clips() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(seed(tmp.path(), 120, NOW).unwrap(), 120);
        let clock: Arc<dyn Clock> = Arc::new(SeedClock(AtomicI64::new(NOW.0)));
        let s = Storage::open(tmp.path(), OpenMode::ReadOnly, clock).unwrap();
        let (rows, next) = s.search(&recent(200)).unwrap();
        assert_eq!(rows.len(), 120);
        assert!(next.is_none());
        // Newest first: clip 0 was copied last.
        assert_eq!(
            rows[0].preview,
            darkclip_core::preview::make_preview(&demo_text(0))
        );
        let sources: HashSet<Option<String>> = rows.iter().map(|r| r.source_app.clone()).collect();
        assert_eq!(sources.len(), 3);
        // Seeding again adds the same clips again (bumped, not duplicated).
        assert_eq!(seed(tmp.path(), 120, NOW).unwrap(), 120);
        assert_eq!(s.search(&recent(200)).unwrap().0.len(), 120);
    }
}
