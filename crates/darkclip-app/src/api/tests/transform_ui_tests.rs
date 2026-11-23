//! Task 404: transforms, "Copy as" and "Save as new clip" through the App.

use super::*;
use darkclip_core::policy::SkipReason;
use darkclip_core::transform::{Transform, TransformError, MAX_INPUT_BYTES};
use darkclip_core::{CaptureMode, ClipType, SensitivityLevel};
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
    assert!(!format!("{p:?}").contains("VIỆT"));
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
    assert!(a.ui_clip_detail(&ui(), id).unwrap().masked);
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
    assert!(!page.items[0].preview.contains("aB3dE5gH7jK9mN1pQ2"));
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

/// The stored clip with its bytes, for "nothing was touched" comparisons.
fn snapshot(a: &App, id: ClipId) -> (darkclip_core::Clip, Vec<u8>) {
    let st = a.reader();
    let clip = st.get_clip(&id).unwrap().unwrap();
    let bytes = st.read_representation(&id, 0).unwrap();
    (clip, bytes)
}

fn assert_untouched(before: &(darkclip_core::Clip, Vec<u8>), a: &App, id: ClipId) {
    let now = snapshot(a, id);
    assert_eq!(now.0.copy_count, before.0.copy_count);
    assert_eq!(now.0.use_count, before.0.use_count);
    assert_eq!(now.0.last_copied_at, before.0.last_copied_at);
    assert_eq!(now.0.last_used_at, before.0.last_used_at);
    assert_eq!(now.0.source, before.0.source);
    assert_eq!(now.0.sensitivity, before.0.sensitivity);
    assert_eq!(now.0.expires_at, before.0.expires_at);
    assert_eq!(now.0.representations, before.0.representations);
    assert_eq!(now.0.preview, before.0.preview);
    assert_eq!(now.1, before.1);
}

#[test]
fn saving_unchanged_text_reports_exists_and_touches_nothing() {
    let h = start(&[("original text", false)], None);
    let a = h.app();
    let before = snapshot(a, h.ids[0]);
    assert!(before.0.source.is_some());
    while h.events.try_recv().is_ok() {}
    let out = a.ui_save_edited(&ui(), "original text".into()).unwrap();
    assert_eq!(out, SaveOutcome::Exists(h.ids[0]));
    assert_untouched(&before, a, h.ids[0]);
    assert_eq!(clip_count(a), 1);
    assert!(h.events.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_line_ending_only_edit_reports_exists_and_keeps_the_original_bytes() {
    let h = start(&[("line one\r\nline two", false)], None);
    let a = h.app();
    let before = snapshot(a, h.ids[0]);
    let out = a
        .ui_save_edited(&ui(), "line one\nline two".into())
        .unwrap();
    assert_eq!(out, SaveOutcome::Exists(h.ids[0]));
    assert_untouched(&before, a, h.ids[0]);
    assert_eq!(before.1, b"line one\r\nline two");
    assert_eq!(clip_count(a), 1);
}

#[test]
fn saving_a_stored_secret_under_store_masked_does_not_mask_the_original() {
    // Stored while the setting was "store"; the setting is "store masked" now.
    let mut s = Settings::default();
    s.privacy.sensitive_handling = SensitiveHandling::StoreMasked;
    let h = start(&[(&secret_text(), false)], Some(s));
    let a = h.app();
    let before = snapshot(a, h.ids[0]);
    assert_eq!(before.0.sensitivity.level, SensitivityLevel::None);
    let out = a.ui_save_edited(&ui(), secret_text()).unwrap();
    assert_eq!(out, SaveOutcome::Exists(h.ids[0]));
    let after = snapshot(a, h.ids[0]);
    assert_eq!(after.0.sensitivity.level, SensitivityLevel::None);
    assert_eq!(after.0.expires_at, None);
    assert_untouched(&before, a, h.ids[0]);
}

#[test]
fn save_edited_in_private_mode_is_refused_but_paused_and_disabled_allow_it() {
    let h = start(&[], None);
    let a = h.app();
    a.set_capture_mode(&ui(), CaptureMode::Private).unwrap();
    assert_eq!(
        a.ui_save_edited(&ui(), "private words".into()).unwrap(),
        SaveOutcome::Dropped(SkipReason::Private)
    );
    assert_eq!(clip_count(a), 0);
    a.set_capture_mode(
        &ui(),
        CaptureMode::Paused {
            until: Timestamp(T0 + 3_600_000),
        },
    )
    .unwrap();
    assert!(matches!(
        a.ui_save_edited(&ui(), "paused words".into()).unwrap(),
        SaveOutcome::Saved(_)
    ));
    drop(h);
    let mut s = Settings::default();
    s.privacy.capture_enabled = false;
    let h = start(&[], Some(s));
    assert!(matches!(
        h.app()
            .ui_save_edited(&ui(), "disabled words".into())
            .unwrap(),
        SaveOutcome::Saved(_)
    ));
    assert_eq!(clip_count(h.app()), 1);
}

#[test]
fn an_empty_transform_result_cannot_be_copied() {
    let h = start(&[("   ", false)], None);
    let p = h
        .app()
        .ui_transform_preview(&ui(), h.ids[0], Transform::Trim)
        .unwrap();
    assert_eq!((p.text.as_str(), p.error), ("", None));
    assert!(h
        .app()
        .ui_copy_transformed(&ui(), h.ids[0], Transform::Trim)
        .is_err());
    assert!(h.fake.current().is_empty());
}
