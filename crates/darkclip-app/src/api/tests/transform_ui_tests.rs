//! Task 404: transforms, "Copy as" and "Save as new clip" through the App.

use super::*;
use darkclip_core::policy::SkipReason;
use darkclip_core::transform::{Transform, TransformError, MAX_INPUT_BYTES};
use darkclip_core::ClipType;
use darkclip_storage::SearchQuery;

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

fn ui() -> Caller {
    Caller::Ui
}

#[test]
fn preview_shows_the_output_and_reports_transform_errors() {
    let h = start(&[("Việt Nam", false), ("not json", false)], None);
    let a = h.app();
    let p = a
        .ui_transform_preview(&ui(), h.ids[0], Transform::Upper)
        .unwrap();
    assert_eq!(p.text, "VIỆT NAM");
    assert!(!p.truncated);
    assert_eq!(p.error, None);
    let e = a
        .ui_transform_preview(&ui(), h.ids[1], Transform::JsonPretty)
        .unwrap();
    assert_eq!(e.error, Some(TransformError::InvalidJson));
    assert_eq!(e.text, "");
    assert_eq!(format!("{p:?}").contains("VIỆT"), false);
}

#[test]
fn preview_is_cut_like_the_ui_text() {
    let big = "€".repeat(20_000);
    let h = start(&[(&big, false)], None);
    let p = h
        .app()
        .ui_transform_preview(&ui(), h.ids[0], Transform::Lower)
        .unwrap();
    assert!(p.truncated);
    assert!(p.text.len() <= UI_TEXT_MAX_BYTES && p.text.len().is_multiple_of(3));
}

#[test]
fn transforms_refuse_masked_images_other_callers_and_expired() {
    let h = start(&[("secret", true), ("plain", false)], None);
    let a = h.app();
    let masked = h.ids[0];
    assert_eq!(
        a.ui_transform_preview(&ui(), masked, Transform::Upper)
            .unwrap_err(),
        ApiError::NotFound
    );
    assert_eq!(
        a.ui_copy_transformed(&ui(), masked, Transform::Upper)
            .unwrap_err(),
        ApiError::NotFound
    );
    assert_eq!(
        a.ui_edit_text(&ui(), masked).unwrap_err(),
        ApiError::NotFound
    );
    assert!(h.fake.current().is_empty());
    for caller in [mcp(), Caller::Cli] {
        let denied = |e: ApiError| matches!(e, ApiError::PermissionDenied(_));
        assert!(denied(
            a.ui_transform_preview(&caller, h.ids[1], Transform::Upper)
                .unwrap_err()
        ));
        assert!(denied(
            a.ui_copy_transformed(&caller, h.ids[1], Transform::Upper)
                .unwrap_err()
        ));
        assert!(denied(a.ui_edit_text(&caller, h.ids[1]).unwrap_err()));
        assert!(denied(a.ui_save_edited(&caller, "x".into()).unwrap_err()));
    }
    let missing = ClipId::new_v7(Timestamp(T0));
    assert_eq!(
        a.ui_transform_preview(&ui(), missing, Transform::Upper)
            .unwrap_err(),
        ApiError::NotFound
    );
    drop(h);
    let (h, _) = start_with_clips_clock(&[image_clip(8, 8, false), files_clip("file:///a/b.txt")]);
    for id in &h.ids {
        assert_eq!(
            h.app()
                .ui_transform_preview(&ui(), *id, Transform::Upper)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            h.app().ui_edit_text(&ui(), *id).unwrap_err(),
            ApiError::NotFound
        );
    }
    let mut expiring = new_clip("expiring", false);
    expiring.expires_at = Some(Timestamp(T0 + 60_000));
    let (h, clock) = start_with_clips_clock(&[expiring]);
    clock.0.store(T0 + 2 * 3_600_000, Ordering::SeqCst);
    assert_eq!(
        h.app()
            .ui_transform_preview(&ui(), h.ids[0], Transform::Upper)
            .unwrap_err(),
        ApiError::NotFound
    );
}

#[test]
fn oversize_text_is_refused_not_cut() {
    let big = "a".repeat(MAX_INPUT_BYTES + 10);
    let h = start(&[(&big, false)], None);
    let p = h
        .app()
        .ui_transform_preview(&ui(), h.ids[0], Transform::Upper)
        .unwrap();
    assert_eq!(p.error, Some(TransformError::TooLarge));
    assert_eq!(
        h.app()
            .ui_copy_transformed(&ui(), h.ids[0], Transform::Upper)
            .unwrap_err(),
        ApiError::InvalidArgument("transform failed")
    );
    assert!(h.fake.current().is_empty());
}

#[test]
fn copy_as_writes_the_output_and_never_creates_a_clip() {
    let h = start(&[("hello world", false)], None);
    let a = h.app();
    while h.events.try_recv().is_ok() {}
    a.ui_copy_transformed(&ui(), h.ids[0], Transform::Upper)
        .unwrap();
    let cur = h.fake.current();
    assert_eq!(cur.len(), 1);
    assert_eq!(cur[0].mime, Mime::TextPlain);
    assert_eq!(cur[0].bytes, b"HELLO WORLD");
    assert_eq!(h.fake.read_calls(), 0);
    // Our own write is not captured again, and no clip was stored.
    let seen: Vec<AppEvent> =
        std::iter::from_fn(|| h.events.recv_timeout(Duration::from_millis(300)).ok()).collect();
    assert!(
        seen.iter()
            .all(|e| !matches!(e, AppEvent::ClipAdded(_) | AppEvent::ClipBumped(_))),
        "{seen:?}"
    );
    assert_eq!(clip_count(a), 1);
    assert_eq!(a.ui_clip_text(&ui(), h.ids[0]).unwrap().text, "hello world");
    // A failing transform writes nothing.
    assert!(a
        .ui_copy_transformed(&ui(), h.ids[0], Transform::Base64Decode)
        .is_err());
    assert_eq!(h.fake.current()[0].bytes, b"HELLO WORLD");
}

#[test]
fn edit_text_returns_more_than_the_display_cap_but_not_unbounded() {
    let big = "€".repeat(20_000);
    let h = start(&[(&big, false), (&"b".repeat(300 * 1024), false)], None);
    let a = h.app();
    assert_eq!(a.ui_edit_text(&ui(), h.ids[0]).unwrap(), big);
    assert_eq!(
        a.ui_edit_text(&ui(), h.ids[1]).unwrap_err(),
        ApiError::InvalidArgument("too large to edit")
    );
}

#[test]
fn save_edited_creates_a_new_clip_and_leaves_the_original() {
    let h = start(&[("original text", false)], None);
    let a = h.app();
    while h.events.try_recv().is_ok() {}
    let out = a.ui_save_edited(&ui(), "edited text ✓".into()).unwrap();
    let SaveOutcome::Saved(new_id) = out else {
        panic!("{out:?}");
    };
    assert_ne!(new_id, h.ids[0]);
    assert_eq!(clip_count(a), 2);
    assert_eq!(a.ui_clip_text(&ui(), new_id).unwrap().text, "edited text ✓");
    assert_eq!(
        a.ui_clip_text(&ui(), h.ids[0]).unwrap().text,
        "original text"
    );
    let ev = h.events.recv_timeout(WAIT).unwrap();
    assert_eq!(ev, AppEvent::ClipAdded(new_id));
    // The clipboard was not touched.
    assert!(h.fake.current().is_empty());
}

#[test]
fn save_edited_secret_is_dropped_under_default_settings() {
    let h = start(&[("original", false)], None);
    let a = h.app();
    let out = a.ui_save_edited(&ui(), secret_text()).unwrap();
    assert_eq!(out, SaveOutcome::Dropped(SkipReason::Sensitive));
    assert_eq!(clip_count(a), 1);
    assert_eq!(a.ui_clip_text(&ui(), h.ids[0]).unwrap().text, "original");
}

#[test]
fn save_edited_follows_the_sensitive_handling_setting() {
    let mut s = Settings::default();
    s.privacy.sensitive_handling = SensitiveHandling::StoreMasked;
    let h = start(&[], Some(s));
    let a = h.app();
    let SaveOutcome::Saved(id) = a.ui_save_edited(&ui(), secret_text()).unwrap() else {
        panic!("not saved");
    };
    // Stored masked: the preview is not the text, mcp cannot see it, the UI list can.
    assert!(a.get_clip(&mcp(), id, GetOptions::default()).is_err());
    assert_eq!(a.ui_clip_detail(&ui(), id).unwrap().masked, true);
    let page = a
        .search_clips(
            &ui(),
            SearchQuery {
                include_masked: true,
                ..query(None)
            },
        )
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.items[0].preview.contains(concat!("s", "k-")));
}

#[test]
fn save_edited_validates_size_and_emptiness_and_kind() {
    let h = start(&[], None);
    let a = h.app();
    assert_eq!(
        a.ui_save_edited(&ui(), "  \n ".into()).unwrap_err(),
        ApiError::InvalidArgument("nothing to save")
    );
    let too_big = "a".repeat(1024 * 1024 + 1);
    assert_eq!(
        a.ui_save_edited(&ui(), too_big).unwrap_err(),
        ApiError::InvalidArgument("too large")
    );
    assert_eq!(clip_count(a), 0);
}

#[test]
fn save_edited_respects_disabled_kinds() {
    let mut s = Settings::default();
    s.privacy.capture_kinds = vec![ClipType::Image];
    let h = start(&[], Some(s));
    assert_eq!(
        h.app().ui_save_edited(&ui(), "plain words".into()).unwrap(),
        SaveOutcome::Dropped(SkipReason::KindDisabled)
    );
    assert_eq!(clip_count(h.app()), 0);
}
