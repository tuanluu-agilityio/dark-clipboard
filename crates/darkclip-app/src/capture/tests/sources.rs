use super::*;

#[test]
fn excluded_app_after_other_copy_in_window_is_skipped() {
    in_one_window(|| {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            slow_cfg(),
            |_| {},
        );
        let mut c = text_copy("notepad text");
        c.source = Some(src("notepad.exe"));
        h.fake.copy(c);
        // The signal carries notepad.exe (resolved at signal time, F-01), so no sync with
        // the actor's dequeue is needed before the next copy.
        h.fake.copy(excluded_copy("vault entry"));
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        // Every attempt, also one whose window closed early: the excluded copy is never stored.
        assert!(!h.stored_previews().iter().any(|p| p == "vault entry"));
        if h.fake.snapshot_meta_calls() > 1 {
            return false; // Window closed between the copies: notepad's was inspected alone.
        }
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
        assert_eq!(h.clip_count(), 0);
        true
    });
}

#[test]
fn other_copy_after_excluded_app_in_window_is_not_mistagged() {
    in_one_window(|| {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            slow_cfg(),
            |_| {},
        );
        // The signal carries the excluded owner; no sync with the dequeue is needed.
        h.fake.copy(excluded_copy("vault entry"));
        let mut c = text_copy("notepad text");
        c.source = Some(src("notepad.exe"));
        h.fake.copy(c);
        let (id, before) = h.wait_added();
        // Never mis-tagged, whether or not the window held.
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.preview, "notepad text");
        assert_eq!(
            clip.source.as_ref().map(|s| s.app_id.as_str()),
            Some("notepad.exe")
        );
        if h.fake.snapshot_meta_calls() > 1 {
            return false; // Window closed: the excluded change was inspected alone.
        }
        // One window: the excluded change was superseded, never inspected on its own.
        assert!(before.is_empty(), "{before:?}");
        true
    });
}

#[test]
fn excluded_app_matches_any_source_of_the_change() {
    let policy = PrivacyPolicy::default();
    let (notepad, vault) = (src("notepad.exe"), src("KeePassXC.exe"));
    let available = [AvailableFormat {
        mime: Mime::TextPlain,
        platform_format: "CF_UNICODETEXT".into(),
        size_hint: Some(4),
    }];
    let decide = |source: Option<&ClipboardSource>,
                  markers: &[PrivacyMarker],
                  others: &[&ClipboardSource]| {
        decide_all_sources(
            &PreReadInput {
                seq: 9,
                last_self_write_seq: None,
                mode: &policy.mode,
                now: Timestamp(T0),
                capture_enabled: true,
                markers,
                source,
                excluded: &policy.excluded_apps,
                available: &available,
                enabled_kinds: &policy.capture_kinds,
                caps: caps(),
            },
            others,
        )
    };
    assert_eq!(
        decide(Some(&notepad), &[], &[&vault]),
        Decision::Skip(SkipReason::ExcludedApp)
    );
    assert_eq!(
        decide(None, &[], &[&vault]),
        Decision::Skip(SkipReason::ExcludedApp)
    );
    // Precedence unchanged: a mandatory marker still reports as concealed.
    assert_eq!(
        decide(
            Some(&notepad),
            &[PrivacyMarker::PasswordManagerHint],
            &[&vault]
        ),
        Decision::Skip(SkipReason::OsConcealed)
    );
    assert!(matches!(
        decide(Some(&notepad), &[], &[]),
        Decision::Read(_)
    ));
}

#[test]
fn excluded_source_seen_at_any_point_of_the_change_skips() {
    // The first-signal source is excluded, the snapshot's owner is not (or is gone).
    let h = start(PrivacyPolicy::default());
    // The signal carries the excluded owner (F-01); the owner may be gone at dequeue.
    h.fake.copy(excluded_copy("vault entry"));
    h.fake.clear_source();
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
}

#[test]
fn signal_source_is_primary_over_dequeue_resolve() {
    // The signal carries A (resolved on the monitor thread); the owner is B by the
    // time the actor dequeues and snapshots.
    let h = start(PrivacyPolicy::default());
    let mut c = text_copy("from app a");
    c.source = Some(src("a.exe"));
    h.fake.copy_then_set_source(c, Some(src("b.exe")));
    let (id, _) = h.wait_added();
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(
        clip.source.as_ref().map(|s| s.app_id.as_str()),
        Some("a.exe")
    );
    // The signal had a source: no resolve at dequeue.
    assert_eq!(h.fake.resolve_calls(), 0);
    // B is still checked against excluded apps.
    let mut c = text_copy("vault entry");
    c.source = Some(src("a.exe"));
    h.fake.copy_then_set_source(c, Some(src("KeePassXC.exe")));
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 1);
    assert_eq!(h.fake.resolve_calls(), 0);
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn excluded_source_in_signal_skips() {
    // Only the signal knows the excluded owner; it is gone at dequeue and snapshot.
    let h = start(PrivacyPolicy::default());
    h.fake
        .copy_then_set_source(excluded_copy("vault entry"), None);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.fake.resolve_calls(), 0);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn signal_without_source_falls_back_to_dequeue_resolve() {
    // Backends that cannot resolve at signal time: the actor resolves at dequeue.
    let h = start(PrivacyPolicy::default());
    h.fake
        .copy_then_set_source(text_copy("late owner"), Some(src("excel.exe")));
    let (id, _) = h.wait_added();
    assert!(h.fake.resolve_calls() >= 1);
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(
        clip.source.as_ref().map(|s| s.app_id.as_str()),
        Some("excel.exe")
    );
}
