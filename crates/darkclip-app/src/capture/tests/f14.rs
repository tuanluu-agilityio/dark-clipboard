use super::*;

// F-14 (Task 109): OLE writers bump the seq at each phase; the final flush has no owner.

/// Debounce wide enough that signal gaps in these tests stay inside it; the 500 ms burst
/// cap still bounds every wait.
fn ole_cfg() -> CaptureConfig {
    CaptureConfig {
        debounce_ms: 1_000,
        ..test_cfg()
    }
}

/// A copy with an owner, offering the OLE placeholder next to its text.
fn ole_owned(app: &str, text: &str) -> FakeCopy {
    FakeCopy {
        formats: vec![
            (pf("DataObject", None, 8), Vec::new()),
            (
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), text.len()),
                text.as_bytes().to_vec(),
            ),
        ],
        markers: Vec::new(),
        source: Some(src(app)),
    }
}

/// The ownerless flush phase: the OLE placeholders and the rendered text, no owner.
fn ole_flush(text: &str) -> FakeCopy {
    FakeCopy {
        formats: vec![
            (pf("DataObject", None, 8), Vec::new()),
            (pf("Ole Private Data", None, 8), Vec::new()),
            (
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), text.len()),
                text.as_bytes().to_vec(),
            ),
        ],
        markers: Vec::new(),
        source: None,
    }
}

fn app_of(h: &H, id: &ClipId) -> Option<String> {
    h.ro()
        .get_clip(id)
        .unwrap()
        .unwrap()
        .source
        .map(|s| s.app_id)
}

#[test]
fn ole_flush_follow_up_keeps_source() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake.copy_burst(vec![
        ole_owned("notepad.exe", "flushed text"),
        ole_flush("flushed text"),
    ]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(app_of(&h, &id).as_deref(), Some("notepad.exe"));
    assert_eq!(h.fake.snapshot_meta_calls(), 1);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn excluded_app_ole_flush_without_markers_is_skipped() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    // KeePassXC.exe is on the default excluded list; no markers on either phase.
    h.fake.copy_burst(vec![
        ole_owned("KeePassXC.exe", "vault entry"),
        ole_flush("vault entry"),
    ]);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "vault entry"));
}

#[test]
fn ownerless_write_without_ole_signature_does_not_inherit() {
    // Not excluded: stored, but the source is not inherited.
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake.copy_burst(vec![
        ole_owned("notepad.exe", "first writer"),
        text_copy("other writer"),
    ]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "other writer");
    assert!(clip.source.is_none());
    // Excluded: the superseded source is still checked, so the change is skipped unread.
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake.copy_burst(vec![
        ole_owned("KeePassXC.exe", "vault entry"),
        text_copy("other writer"),
    ]);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
}

#[test]
fn processed_change_is_never_carried() {
    // A short burst cap processes the first change long before the debounce would.
    let h = start_full(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        Duration::from_millis(20),
        |_| {},
    );
    h.fake.copy(ole_owned("notepad.exe", "first text"));
    let (first, _) = h.wait_added();
    assert_eq!(app_of(&h, &first).as_deref(), Some("notepad.exe"));
    // Within the debounce of the first signal, OLE signature, seq + 1: still not carried.
    h.fake.copy(ole_flush("second text"));
    let (second, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(app_of(&h, &second), None);
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn self_write_seq_breaks_the_chain() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake.copy(text_copy("stored earlier"));
    let (earlier, _) = h.wait_added();
    h.fake.copy(ole_owned("notepad.exe", "before copy back"));
    h.copy_back(earlier, false).unwrap();
    h.fake.copy(ole_flush("after copy back"));
    let (id, before) = h.wait_added();
    assert!(
        before.contains(&AppEvent::Skipped(SkipReason::SelfWrite)),
        "{before:?}"
    );
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "after copy back");
    assert!(clip.source.is_none());
}

#[test]
fn burst_cap_bounds_the_chain() {
    // Signal times are explicit; both signals are queued together, so only their times
    // differ. Debounce 1 s, burst cap 500 ms.
    let run = |gap_ms: i64| {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        h.fake.copy_burst_at(vec![
            (Timestamp(T0), ole_owned("notepad.exe", "flushed text")),
            (Timestamp(T0 + gap_ms), ole_flush("flushed text")),
        ]);
        let (id, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        assert_eq!(h.fake.snapshot_meta_calls(), 1);
        app_of(&h, &id)
    };
    // Inside the cap: inherited.
    assert_eq!(run(400).as_deref(), Some("notepad.exe"));
    // Beyond the cap (but inside the debounce): not inherited.
    assert_eq!(run(600), None);
}

// F-14 rule 3 (D-042): an ownerless change shortly after an excluded-app skip.

#[test]
fn excluded_app_processed_then_ownerless_flush_is_skipped() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy_burst_at(vec![(
        Timestamp(T0),
        ole_owned("KeePassXC.exe", "vault entry"),
    )]);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    // The excluded change was inspected and skipped; then the ownerless flush comes,
    // 20 ms later by signal time.
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 20), ole_flush("vault entry"))]);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    // Sentinel with an owner (an ownerless one this soon is skipped too, by design).
    h.fake.copy(ole_owned("notepad.exe", "sentinel clip"));
    let (_, between) = h.wait_added();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn ownerless_change_after_window_is_not_affected() {
    // Beyond the 500 ms cap from the excluded change's signal.
    let h = start(PrivacyPolicy::default());
    h.fake
        .copy_burst_at(vec![(Timestamp(T0), ole_owned("KeePassXC.exe", "vault"))]);
    h.wait_skip(SkipReason::ExcludedApp);
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 600), ole_flush("later text"))]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(app_of(&h, &id), None);
    // A self-write in between clears it, even inside the window.
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("stored earlier"));
    let (earlier, _) = h.wait_added();
    h.fake.copy(ole_owned("KeePassXC.exe", "vault"));
    h.wait_skip(SkipReason::ExcludedApp);
    h.copy_back(earlier, false).unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    h.fake.copy(ole_flush("after copy back"));
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "after copy back");
    assert!(clip.source.is_none());
}

#[test]
fn owned_change_after_excluded_skip_is_not_affected() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(ole_owned("KeePassXC.exe", "vault entry"));
    h.wait_skip(SkipReason::ExcludedApp);
    h.fake.copy(ole_owned("notepad.exe", "notepad text"));
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(app_of(&h, &id).as_deref(), Some("notepad.exe"));
}

// Security review of Task 109.

#[test]
fn excluded_owner_then_flush_across_actor_stall_is_skipped() {
    let h = start(PrivacyPolicy::default());
    // The actor is preempted for longer than the burst cap when it dequeues the flush.
    *h.cap().hooks.stall_on_signal.lock().unwrap() = Some((2, Duration::from_millis(700)));
    let last = h.fake.copy_burst_at(vec![
        (Timestamp(T0), ole_owned("KeePassXC.exe", "vault entry")),
        (Timestamp(T0 + 20), ole_flush("vault entry")),
    ]);
    assert_eq!(last, 2);
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.fake.snapshot_meta_calls(), 1);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn inspected_change_is_never_heir() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    // Phase 1: placeholders only, owner known: snapshotted, nothing readable (carried).
    h.fake.copy(FakeCopy {
        formats: vec![
            (pf("DataObject", None, 8), Vec::new()),
            (pf("Ole Private Data", None, 8), Vec::new()),
        ],
        markers: Vec::new(),
        source: Some(src("notepad.exe")),
    });
    h.wait_skip(SkipReason::NothingReadable);
    // Signalled again (same seq, new pending), then replaced by an ownerless OLE flush
    // in the same burst: the replaced change was inspected, so it is not the heir.
    h.fake.signal_again(1);
    h.fake.copy(ole_flush("flushed text"));
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(h.fake.snapshot_meta_calls(), 2);
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "flushed text");
    assert!(clip.source.is_none());
}

#[test]
fn carried_retry_still_checks_superseded_sources() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    let placeholders = |source: Option<ClipboardSource>| FakeCopy {
        formats: vec![
            (pf("DataObject", None, 8), Vec::new()),
            (pf("Ole Private Data", None, 8), Vec::new()),
        ],
        markers: Vec::new(),
        source,
    };
    // Owner B is superseded in the burst by an ownerless, placeholder-only change that
    // cannot inherit B (600 ms apart by signal time, beyond the cap): B is only checked
    // (rule 1). Nothing readable yet, so the seq is carried.
    h.fake.copy_burst_at(vec![
        (Timestamp(T0), placeholders(Some(src("tool.exe")))),
        (Timestamp(T0 + 600), placeholders(None)),
    ]);
    h.wait_skip(SkipReason::NothingReadable);
    // B becomes excluded before the same seq renders real formats.
    let mut policy = PrivacyPolicy::default();
    policy.excluded_apps.push(ExcludedApp {
        app_id: "tool.exe".into(),
        platform: Platform::Windows,
        name: None,
        builtin: false,
    });
    h.cap()
        .send(ActorCommand::UpdatePolicy(
            policy,
            RetentionPolicy::default(),
        ))
        .unwrap();
    h.fake.render(ole_flush("rendered text").formats);
    h.fake.signal_again(1);
    // The retry of the carried seq still checks B.
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.clip_count(), 0);
}

// Phase 2 review (Task 110, D-043).

#[test]
fn newer_snapshot_seq_does_not_inherit_old_owner() {
    // Owner A's change is signalled; before the actor looks, owner B's copy replaces it
    // with a higher seq (B's signal not delivered yet). The snapshot is B's: stored with
    // B as source, never A.
    let h = start(PrivacyPolicy::default());
    let seq = h.fake.copy_then_replace_silently(
        ole_owned("tool-a.exe", "text of a"),
        ole_owned("tool-b.exe", "text of b"),
    );
    assert_eq!(seq, 2);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "text of b");
    assert_eq!(app_of(&h, &id).as_deref(), Some("tool-b.exe"));

    // B has no owner at snapshot time: stored without a source, never A (even in the
    // OLE flush shape, since the snapshot's seq is not the signalled one).
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake
        .copy_then_replace_silently(ole_owned("tool-a.exe", "text of a"), ole_flush("text of b"));
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(app_of(&h, &id), None);

    // A is still checked for exclusion: an excluded A skips the change, nothing read.
    let h = start(PrivacyPolicy::default());
    h.fake.copy_then_replace_silently(
        ole_owned("KeePassXC.exe", "vault entry"),
        ole_owned("tool-b.exe", "text of b"),
    );
    let seen = h.wait_skip(SkipReason::ExcludedApp);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn clock_step_back_disables_rule_two_but_not_rule_three() {
    let (t, back) = (Timestamp(T0), Timestamp(T0 - 5_000));
    // Rule 2 (promotion): a negative gap is never "within".
    assert!(!within_ms(t, back, 500));
    assert!(within_ms(t, Timestamp(T0 + 500), 500));
    assert!(!within_ms(t, Timestamp(T0 + 501), 500));
    // Rule 3 (skip only): a negative gap counts as none.
    assert!(within_ms_or_earlier(t, back, 500));
    assert!(within_ms_or_earlier(t, Timestamp(T0 + 500), 500));
    assert!(!within_ms_or_earlier(t, Timestamp(T0 + 501), 500));
}
