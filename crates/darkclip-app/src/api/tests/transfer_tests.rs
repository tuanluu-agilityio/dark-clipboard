//! Task 405: backup, restore, export and import through the App.

use super::*;
use darkclip_core::policy::SkipReason;
use darkclip_core::{ClipType, CollectionKind, CollectionSort, SchemaVersion, SensitiveHandling};
use darkclip_storage::ArchiveLimits;
use std::io::{Read, Write};
use std::path::PathBuf;

fn ui() -> Caller {
    Caller::Ui
}

fn secret_text() -> String {
    // Runtime-assembled (GC-09).
    let body: String = "aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0z"
        .chars()
        .cycle()
        .take(48)
        .collect();
    format!("key {}{}", concat!("s", "k-"), body)
}

fn clip_count(a: &App) -> u64 {
    a.get_storage_stats(&Caller::Ui).unwrap().clips
}

fn texts(a: &App) -> Vec<String> {
    let page = a.search_clips(&ui(), query(None)).unwrap();
    let mut v: Vec<String> = page.items.into_iter().map(|i| i.preview).collect();
    v.sort();
    v
}

fn png(seed: u8) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[seed; 16]).unwrap();
    }
    out
}

fn image_clip(seed: u8) -> NewClip {
    let reps = vec![RawRepresentation {
        mime: Mime::ImagePng,
        platform_format: "PNG".into(),
        bytes: png(seed),
        width: Some(2),
        height: Some(2),
        truncated: false,
    }];
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

/// An export file made by a separate store holding `clips` (all exported).
fn export_file(dir: &Path, clips: Vec<NewClip>) -> PathBuf {
    let clock: Arc<dyn Clock> = Arc::new(TestClock(AtomicI64::new(T0 - 86_400_000)));
    let src = dir.join("source-store");
    let mut s = Storage::open(&src, OpenMode::ReadWrite, clock).unwrap();
    let mut ids = Vec::new();
    for n in clips {
        match s.upsert_clip(&n).unwrap() {
            darkclip_storage::UpsertOutcome::Inserted(id)
            | darkclip_storage::UpsertOutcome::Bumped(id) => ids.push(id),
        }
    }
    let col = darkclip_core::Collection {
        schema_version: SchemaVersion,
        id: CollectionId::new_v7(Timestamp(T0)),
        name: "Shared snippets".into(),
        kind: CollectionKind::User,
        system_key: None,
        color: None,
        created_at: Timestamp(T0),
        updated_at: Timestamp(T0),
        clip_count: 0,
        sort: CollectionSort::Manual,
    };
    let out = dir.join("shared.dcexport");
    s.write_export(&ids, &col, &out, "0.0.1").unwrap();
    out
}

/// Rewrites `export.json` inside an export file.
fn edit_export(path: &Path, f: impl FnOnce(&mut serde_json::Value)) {
    let mut entries = Vec::new();
    {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        for i in 0..z.len() {
            let mut e = z.by_index(i).unwrap();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            entries.push((e.name().unwrap().to_string(), b));
        }
    }
    let j = entries
        .iter_mut()
        .find(|(n, _)| n == "export.json")
        .unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&j.1).unwrap();
    f(&mut v);
    j.1 = serde_json::to_vec(&v).unwrap();
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in &entries {
        z.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap();
}

fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = Settings::default();
    f(&mut s);
    s
}

fn exported_collection(a: &App) -> CollectionView {
    a.list_collections(&ui())
        .unwrap()
        .into_iter()
        .find(|c| !c.system)
        .unwrap()
}

#[test]
fn backup_and_restore_through_the_app() {
    let h = start(&[("first clip", false), ("second clip", false)], None);
    let a = h.app();
    let out = h.dir.path().join("mine.dcbackup");
    assert_eq!(a.ui_backup(&ui(), &out).unwrap().clips, 2);
    a.delete_clip(&ui(), h.ids[0]).unwrap();
    assert_eq!(clip_count(a), 1);
    assert_eq!(a.ui_restore(&ui(), &out, FREE).unwrap().clips, 2);
    // The reader sees the restored data at once.
    assert_eq!(texts(a), ["first clip", "second clip"]);
    // Capture keeps working on the restored store.
    h.fake.copy(text_copy("after restore"));
    h.wait_added();
    assert_eq!(clip_count(a), 3);
}

#[test]
fn backup_restore_export_and_import_are_ui_only() {
    let h = start(&[("x", false)], None);
    let a = h.app();
    let p = h.dir.path().join("f");
    for c in [Caller::Cli, mcp()] {
        assert!(matches!(
            a.ui_backup(&c, &p),
            Err(ApiError::PermissionDenied(_))
        ));
        assert!(matches!(
            a.ui_restore(&c, &p, FREE),
            Err(ApiError::PermissionDenied(_))
        ));
        assert!(matches!(
            a.ui_export(&c, ExportSelection::Clips(vec![h.ids[0]]), &p),
            Err(ApiError::PermissionDenied(_))
        ));
        assert!(matches!(
            a.ui_import(&c, &p),
            Err(ApiError::PermissionDenied(_))
        ));
    }
    assert!(!p.exists());
}

#[test]
fn restoring_a_newer_backup_is_refused_and_keeps_the_history() {
    let h = start(&[("keep me", false)], None);
    let a = h.app();
    let out = h.dir.path().join("mine.dcbackup");
    a.ui_backup(&ui(), &out).unwrap();
    // Rewrite the manifest with a newer format.
    let mut entries = Vec::new();
    {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        for i in 0..z.len() {
            let mut e = z.by_index(i).unwrap();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            entries.push((e.name().unwrap().to_string(), b));
        }
    }
    let m = entries
        .iter_mut()
        .find(|(n, _)| n == "manifest.json")
        .unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&m.1).unwrap();
    v["format"] = serde_json::json!(7);
    m.1 = serde_json::to_vec(&v).unwrap();
    let newer = h.dir.path().join("newer.dcbackup");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&newer).unwrap());
    for (name, bytes) in &entries {
        z.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap();
    a.delete_clip(&ui(), h.ids[0]).unwrap();
    h.fake.copy(text_copy("newest"));
    h.wait_added();
    assert_eq!(
        a.ui_restore(&ui(), &newer, FREE),
        Err(ApiError::SchemaTooNew)
    );
    assert_eq!(texts(a), ["newest"]);
    // A file that is not a backup at all.
    let junk = h.dir.path().join("junk.dcbackup");
    std::fs::write(&junk, b"nope").unwrap();
    assert!(matches!(
        a.ui_restore(&ui(), &junk, FREE),
        Err(ApiError::InvalidArgument(_))
    ));
    assert_eq!(texts(a), ["newest"]);
}

#[test]
fn export_leaves_out_masked_clips_and_reports_how_many() {
    let h = start(&[("plain words", false), ("secret", true)], None);
    let a = h.app();
    let out = h.dir.path().join("picked.dcexport");
    let r = a
        .ui_export(&ui(), ExportSelection::Clips(h.ids.clone()), &out)
        .unwrap();
    assert_eq!((r.exported, r.excluded_sensitive), (1, 1));
    let f = darkclip_storage::ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    assert_eq!(f.clips().len(), 1);
    assert_eq!(f.clips()[0].id, h.ids[0]);
}

#[test]
fn export_collection_for_the_ui_writes_under_the_exports_folder() {
    let h = start(&[("member one", false), ("member two", false)], None);
    let a = h.app();
    let col = a.create_collection(&ui(), "Notes").unwrap();
    for id in &h.ids {
        a.add_clip_to_collection(&ui(), *id, col.id).unwrap();
    }
    let r = a.export_collection(&ui(), col.id).unwrap();
    assert_eq!(r.clip_count, 2);
    assert_eq!(r.excluded_sensitive, 0);
    let path = PathBuf::from(&r.path);
    assert!(path.is_file());
    assert_eq!(path.parent().unwrap(), h.dir.path().join("exports"));
    assert_eq!(path.extension().unwrap(), "dcexport");
    // Through the generic export too, by collection.
    let out = h.dir.path().join("notes.dcexport");
    let r = a
        .ui_export(&ui(), ExportSelection::Collection(col.id), &out)
        .unwrap();
    assert_eq!(r.exported, 2);
    let f = darkclip_storage::ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    assert_eq!(f.collection().name, "Notes");
    let missing = CollectionId::new_v7(Timestamp(T0));
    assert_eq!(
        a.export_collection(&ui(), missing).unwrap_err(),
        ApiError::NotFound
    );
}

#[test]
fn import_dedupes_by_content_and_fills_a_collection() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(
        tmp.path(),
        vec![
            new_clip("alpha", false),
            new_clip("beta", false),
            image_clip(5),
        ],
    );
    let h = start(&[("alpha", false)], None);
    let a = h.app();
    let r = a.ui_import(&ui(), &file).unwrap();
    assert_eq!(r.imported, 2);
    assert_eq!(r.merged, 1);
    assert_eq!(r.skipped(), 0);
    assert_eq!(clip_count(a), 3, "no second row for the same content");
    let alpha = a.get_clip(&ui(), h.ids[0], GetOptions::default()).unwrap();
    assert_eq!(
        alpha.clip.copy_count, 2,
        "a duplicate bumps the existing clip"
    );
    assert_eq!(
        alpha.clip.source.as_ref().map(|s| s.app_id.as_str()),
        Some("org.example.editor"),
        "the existing clip keeps its own source"
    );
    let col = exported_collection(a);
    assert_eq!(col.name, "Shared snippets");
    assert_eq!(col.clip_count, 3);
    // Importing the same file again adds nothing new and reuses the collection.
    let r = a.ui_import(&ui(), &file).unwrap();
    assert_eq!((r.imported, r.merged), (0, 3));
    assert_eq!(clip_count(a), 3);
    let cols: Vec<_> = a
        .list_collections(&ui())
        .unwrap()
        .into_iter()
        .filter(|c| !c.system)
        .collect();
    assert_eq!(cols.len(), 1);
}

#[test]
fn imported_secret_text_follows_the_sensitive_setting() {
    let tmp = tempfile::tempdir().unwrap();
    // The source store did not mark it (stored before a scan), so the file carries it.
    let file = export_file(tmp.path(), vec![new_clip(&secret_text(), false)]);
    {
        // Default handling: drop.
        let h = start(&[], None);
        let r = h.app().ui_import(&ui(), &file).unwrap();
        assert_eq!((r.imported, r.skipped_sensitive), (0, 1));
        assert_eq!(clip_count(h.app()), 0);
    }
    {
        let s = settings_with(|s| s.privacy.sensitive_handling = SensitiveHandling::StoreMasked);
        let h = start(&[], Some(s));
        let a = h.app();
        let r = a.ui_import(&ui(), &file).unwrap();
        assert_eq!(r.imported, 1);
        let page = a.search_clips(&ui(), query(None)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].masked);
        assert!(!page.items[0].preview.contains(&secret_text()));
        let c = a
            .get_clip(&ui(), page.items[0].id, GetOptions::default())
            .unwrap();
        assert!(
            c.clip.expires_at.is_some(),
            "masked clips expire like captured ones"
        );
        // A masked import is not put into a collection (D-060).
        assert!(a
            .list_collections(&ui())
            .unwrap()
            .iter()
            .all(|c| c.clip_count == 0));
    }
}

#[test]
fn imported_metadata_is_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip("plain words here", false)]);
    edit_export(&file, |v| {
        let c = &mut v["clips"][0];
        c["kind"] = serde_json::json!("url");
        c["preview"] = serde_json::json!("a preview that lies");
        c["content_hash"] = serde_json::json!("ab".repeat(32));
        c["size_bytes"] = serde_json::json!(999_999);
        c["created_at"] = serde_json::json!("2099-01-01T00:00:00.000Z");
        c["last_copied_at"] = serde_json::json!("2099-01-01T00:00:00.000Z");
        c["copy_count"] = serde_json::json!(500);
        c["use_count"] = serde_json::json!(77);
        c["pinned"] = serde_json::json!(true);
        c["pin_order"] = serde_json::json!(0);
        c["source"] = serde_json::json!({
            "app_id": "evil.exe", "platform": "windows", "confidence": "owner"
        });
        c["representations"][0]["platform_format"] = serde_json::json!("CF_HDROP");
        c["representations"][0]["width"] = serde_json::json!(40000);
    });
    let h = start(&[], None);
    let a = h.app();
    assert_eq!(a.ui_import(&ui(), &file).unwrap().imported, 1);
    let id = a.search_clips(&ui(), query(None)).unwrap().items[0].id;
    let c = a.get_clip(&ui(), id, GetOptions::default()).unwrap().clip;
    assert_eq!(c.kind, ClipType::Text, "kind re-derived");
    assert_eq!(c.preview, "plain words here", "preview re-derived");
    let reps = vec![raw("plain words here")];
    assert_eq!(c.content_hash, canonical_hash(ClipType::Text, &reps, None));
    assert_eq!(c.size_bytes, 16);
    assert_eq!(c.source, None, "source fields dropped");
    assert!(!c.pinned);
    assert_eq!(c.use_count, 0);
    assert_eq!(c.copy_count, 1);
    assert_eq!(c.representations[0].width, None);
    assert_ne!(
        c.representations[0].platform_format.as_deref(),
        Some("CF_HDROP")
    );
    // Future timestamps are clamped to now.
    assert!(c.created_at <= Timestamp(T0));
    assert!(c.last_copied_at <= Timestamp(T0));
}

#[test]
fn past_timestamps_are_kept_and_the_note_and_tags_come_along() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip("with a note", false)]);
    edit_export(&file, |v| {
        let c = &mut v["clips"][0];
        c["created_at"] = serde_json::json!("2026-01-01T00:00:00.000Z");
        c["note"] = serde_json::json!("imported note");
        c["tags"] = serde_json::json!([
            { "id": "019a8f2e-1c00-7a10-8b00-0000000000a1", "name": "Work" },
            { "id": "019a8f2e-1c00-7a10-8b00-0000000000a2", "name": "not a valid tag!" }
        ]);
    });
    let h = start(&[], None);
    let a = h.app();
    assert_eq!(a.ui_import(&ui(), &file).unwrap().imported, 1);
    let id = a.search_clips(&ui(), query(None)).unwrap().items[0].id;
    let c = a.get_clip(&ui(), id, GetOptions::default()).unwrap().clip;
    assert_eq!(
        c.created_at,
        Timestamp::parse_rfc3339("2026-01-01T00:00:00.000Z").unwrap()
    );
    assert_eq!(c.note.as_deref(), Some("imported note"));
    let tags: Vec<&str> = c.tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        tags,
        ["work"],
        "names are normalised; invalid ones are dropped"
    );
}

#[test]
fn import_reports_partial_errors_by_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let long = "z".repeat(3 * 1024);
    let file = export_file(
        tmp.path(),
        vec![
            new_clip("fine", false),
            new_clip(&long, false),
            image_clip(9),
            new_clip("damaged", false),
        ],
    );
    edit_export(&file, |v| {
        // The last clip's data is missing: an inline representation without bytes.
        let r = &mut v["clips"][3]["representations"][0];
        r["storage"] = serde_json::json!("inline");
        r.as_object_mut().unwrap().remove("blob_sha256");
    });
    let s = settings_with(|s| {
        s.retention.text_max_kb = 1;
        s.privacy.capture_kinds.retain(|k| *k != ClipType::Image);
    });
    let h = start(&[], Some(s));
    let a = h.app();
    let r = a.ui_import(&ui(), &file).unwrap();
    assert_eq!(r.imported, 1);
    assert_eq!(r.skipped_too_large, 1);
    assert_eq!(r.skipped_kind, 1);
    assert_eq!(r.skipped_invalid, 1);
    assert_eq!(r.skipped(), 3);
    assert_eq!(texts(a), ["fine"]);
    assert!(!format!("{r:?}").contains("fine"));
}

#[test]
fn import_is_refused_in_private_mode_and_for_bad_files() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip("anything", false)]);
    let h = start(&[], None);
    let a = h.app();
    a.set_capture_mode(&ui(), CaptureMode::Private).unwrap();
    assert!(matches!(
        a.ui_import(&ui(), &file),
        Err(ApiError::InvalidArgument(_))
    ));
    assert_eq!(clip_count(a), 0);
    a.set_capture_mode(&ui(), CaptureMode::Normal).unwrap();
    edit_export(&file, |v| v["schema_version"] = serde_json::json!(5));
    assert_eq!(a.ui_import(&ui(), &file), Err(ApiError::SchemaTooNew));
    let junk = tmp.path().join("junk.dcexport");
    std::fs::write(&junk, b"nope").unwrap();
    assert!(matches!(
        a.ui_import(&ui(), &junk),
        Err(ApiError::InvalidArgument(_))
    ));
    assert_eq!(clip_count(a), 0);
}

#[test]
fn a_deleted_clip_does_not_come_back_without_a_restore() {
    let h = start(&[("gone for good", false), ("stays", false)], None);
    let a = h.app();
    let out = h.dir.path().join("before.dcbackup");
    a.ui_backup(&ui(), &out).unwrap();
    a.delete_clip(&ui(), h.ids[0]).unwrap();
    // Export and backup again: the deleted clip is in neither.
    let exp = h.dir.path().join("after.dcexport");
    let r = a
        .ui_export(&ui(), ExportSelection::Clips(h.ids.clone()), &exp)
        .unwrap();
    assert_eq!(r.exported, 1);
    let out2 = h.dir.path().join("after.dcbackup");
    assert_eq!(a.ui_backup(&ui(), &out2).unwrap().clips, 1);
    assert_eq!(texts(a), ["stays"]);
    // Only an explicit restore of the older backup brings it back.
    a.ui_restore(&ui(), &out, FREE).unwrap();
    assert_eq!(texts(a), ["gone for good", "stays"]);
}

// ---- Task 405 security review ----

/// Free disk space the tests report: plenty.
const FREE: Option<u64> = Some(u64::MAX);

fn reader_of(h: &H) -> Storage {
    let clock: Arc<dyn Clock> = Arc::new(TestClock(AtomicI64::new(T0)));
    Storage::open(h.dir.path(), OpenMode::ReadOnly, clock).unwrap()
}

/// Waits for an event matching `want`.
fn wait_event(h: &H, want: impl Fn(&AppEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match h.events.recv_timeout(left) {
            Ok(e) if want(&e) => return,
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for the event"),
        }
    }
}

#[test]
fn a_no_cloud_copy_is_flagged_and_never_exported() {
    let h = start(&[], None);
    let a = h.app();
    let mut copy = text_copy("the source app said no cloud");
    copy.markers = vec![darkclip_core::markers::PrivacyMarker::NoCloud];
    h.fake.copy(copy);
    let id = h.wait_added();
    assert!(reader_of(&h).is_no_export(&id).unwrap());
    let out = h.dir.path().join("nocloud.dcexport");
    let r = a
        .ui_export(&ui(), ExportSelection::Clips(vec![id]), &out)
        .unwrap();
    assert_eq!((r.exported, r.excluded_sensitive), (0, 1));
    // A backup is the user's own full copy: it keeps the clip.
    let b = h.dir.path().join("nocloud.dcbackup");
    assert_eq!(a.ui_backup(&ui(), &b).unwrap().clips, 1);
}

#[test]
fn import_never_sets_the_no_export_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip("from elsewhere", false)]);
    let h = start(&[], None);
    let a = h.app();
    assert_eq!(a.ui_import(&ui(), &file).unwrap().imported, 1);
    let id = a.search_clips(&ui(), query(None)).unwrap().items[0].id;
    assert!(!reader_of(&h).is_no_export(&id).unwrap());
    // The file cannot carry the flag: an extra field makes it invalid.
    edit_export(&file, |v| {
        v["clips"][0]["no_export"] = serde_json::json!(true)
    });
    assert_eq!(
        a.ui_import(&ui(), &file),
        Err(ApiError::InvalidArgument(crate::events::FILE_INVALID))
    );
}

#[test]
fn a_transfer_does_not_hold_the_caller_and_a_mode_change_applies_before_later_copies() {
    let h = start(&[("before", false)], None);
    let a = h.app();
    let out = h.dir.path().join("slow.dcbackup");
    a.cap.set_transfer_stall(600);
    let t0 = Instant::now();
    let wait = a.ui_start(&ui(), UiTransfer::Backup(out.clone())).unwrap();
    assert!(
        t0.elapsed() < Duration::from_millis(400),
        "ui_start returns while the actor works"
    );
    // While the backup runs: private mode, then a copy. The mode is applied first.
    a.set_capture_mode(&ui(), CaptureMode::Private).unwrap();
    h.fake.copy(text_copy("copied while private"));
    match wait.wait().unwrap() {
        TransferReply::Backup(r) => assert_eq!(r.clips, 1),
        other => panic!("unexpected reply {other:?}"),
    }
    wait_event(&h, |e| *e == AppEvent::Skipped(SkipReason::Private));
    assert_eq!(clip_count(a), 1);
    assert!(out.is_file());
}

#[test]
fn import_checks_the_declared_size_before_reading() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip("tiny", false)]);
    // The file claims a huge blob: refused as too large without reading it.
    edit_export(&file, |v| {
        v["blobs"][0]["size_bytes"] = serde_json::json!(999_999)
    });
    let s = settings_with(|s| s.retention.text_max_kb = 1);
    let h = start(&[], Some(s));
    let r = h.app().ui_import(&ui(), &file).unwrap();
    assert_eq!(
        (r.skipped_too_large, r.skipped_invalid, r.imported),
        (1, 0, 0)
    );
}

#[test]
fn a_merge_onto_a_masked_clip_never_organises_it() {
    let tmp = tempfile::tempdir().unwrap();
    let file = export_file(tmp.path(), vec![new_clip(&secret_text(), false)]);
    let s = settings_with(|s| s.privacy.sensitive_handling = SensitiveHandling::StoreMasked);
    let secret = secret_text();
    let h = start(&[(&secret, true)], Some(s));
    let a = h.app();
    let r = a.ui_import(&ui(), &file).unwrap();
    assert_eq!(r.merged, 1);
    assert!(a
        .list_collections(&ui())
        .unwrap()
        .iter()
        .all(|c| c.clip_count == 0));
}

// ---- Task 405 round-2 review ----

/// `start` with a long debounce, so a copy can still be pending when a transfer begins.
fn start_slow_debounce(debounce_ms: u64) -> H {
    let dir = tempfile::tempdir().unwrap();
    let clock: Arc<dyn Clock> = Arc::new(TestClock(AtomicI64::new(T0)));
    let settings_path = dir.path().join("settings.json");
    Settings::default().save(&settings_path).unwrap();
    let (platform, fake) = FakePlatform::new();
    let (tx, events) = mpsc::channel();
    let cfg = CaptureConfig {
        debounce_ms,
        open_retries: 10,
        retry_delay_ms: 1,
        lang: darkclip_core::Lang::En,
    };
    let (app, _) = App::start(
        platform.bundle(),
        dir.path(),
        &settings_path,
        clock,
        cfg,
        tx,
    )
    .unwrap();
    H {
        dir,
        app: Some(app),
        fake,
        events,
        ids: Vec::new(),
    }
}

#[test]
fn a_change_pending_before_a_transfer_is_not_read_after_a_queued_private_mode() {
    let h = start_slow_debounce(300);
    let a = h.app();
    // A copy whose debounce is still running when the transfer starts.
    h.fake.copy(text_copy("copied before"));
    std::thread::sleep(Duration::from_millis(60));
    a.cap.set_transfer_stall(700);
    let out = h.dir.path().join("slow.dcbackup");
    let wait = a.ui_start(&ui(), UiTransfer::Backup(out)).unwrap();
    // During the transfer: private mode, then another copy (now on the clipboard).
    std::thread::sleep(Duration::from_millis(50));
    a.set_capture_mode(&ui(), CaptureMode::Private).unwrap();
    h.fake.copy(text_copy("copied after private"));
    assert!(matches!(wait.wait(), Ok(TransferReply::Backup(_))));
    // Longer than the debounce: anything still pending has been processed by now.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        texts(a).iter().all(|t| t != "copied after private"),
        "nothing on the clipboard after Private is stored"
    );
}
