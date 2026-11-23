use super::*;

#[test]
fn stores_plain_text() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("hello world"));
    let (id, _) = h.wait_added();
    let ro = h.ro();
    let clip = ro.get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.kind, ClipType::Text);
    assert_eq!(clip.preview, "hello world");
    assert_eq!(clip.copy_count, 1);
    assert_eq!(clip.created_at, Timestamp(T0));
    assert_eq!(ro.read_representation(&id, 0).unwrap(), b"hello world");
    assert_eq!(h.fake.read_calls(), 1);
}

#[test]
fn sensitive_drop_default() {
    let h = start(PrivacyPolicy::default());
    assert_eq!(
        PrivacyPolicy::default().sensitive_handling,
        SensitiveHandling::Drop
    );
    h.fake.copy(text_copy(&api_key_text()));
    let seen = h.wait_skip(SkipReason::Sensitive);
    assert!(no_store_events(&seen), "{seen:?}");
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert_eq!(h.cap().skip_counters().get(SkipReason::Sensitive), 1);
}

#[test]
fn sensitive_store_masked_sets_expiry_and_masked_preview() {
    let policy = PrivacyPolicy {
        sensitive_handling: SensitiveHandling::StoreMasked,
        ..PrivacyPolicy::default()
    };
    let enabled = policy.sensitive_categories_enabled.clone();
    let h = start(policy);
    let text = api_key_text();
    h.fake.copy(text_copy(&text));
    let (id, _) = h.wait_added();
    let ro = h.ro();
    let clip = ro.get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.sensitivity.level, SensitivityLevel::Masked);
    assert!(clip
        .sensitivity
        .categories
        .contains(&SensitiveCategory::ApiKey));
    let ttl = i64::from(RetentionPolicy::default().sensitive_ttl_seconds);
    assert_eq!(clip.expires_at, Some(Timestamp(T0 + ttl * 1000)));
    let hint = scan(&text, &enabled).mask_hint.unwrap();
    assert_eq!(clip.preview, masked_preview(&hint, Lang::En));
    assert!(!clip.preview.contains(&body(48)));
    // The FTS index holds only the masked preview.
    let (rows, _) = ro
        .search(&SearchQuery {
            text: Some(body(20)),
            kinds: Vec::new(),
            pinned_only: false,
            since: None,
            include_masked: true,
            limit: 10,
            cursor: None,
            collection: None,
            tag: None,
        })
        .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn busy_retries_then_drops() {
    let h = start(PrivacyPolicy::default());
    h.fake.set_busy(11);
    h.fake.copy(text_copy("busy clipboard"));
    let seen = h.wait_skip(SkipReason::Busy);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.cap().skip_counters().get(SkipReason::Busy), 1);
    assert_eq!(h.fake.snapshot_meta_calls(), 11);
    assert_eq!(h.fake.read_calls(), 0);
    // Still alive.
    h.sentinel();
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn storage_full_runs_retention_then_retries_once() {
    let h = start(PrivacyPolicy::default());
    h.sentinel();
    let hooks = &h.cap().hooks;
    let before = hooks.attempts.load(Ordering::SeqCst);
    hooks
        .inject
        .lock()
        .unwrap()
        .push_back(StorageError::StorageFull);
    h.fake.copy(text_copy("second clip"));
    let (_, seen) = h.wait_added();
    assert!(
        !seen.iter().any(|e| matches!(e, AppEvent::Unavailable(_))),
        "{seen:?}"
    );
    assert_eq!(hooks.attempts.load(Ordering::SeqCst), before + 1);
    assert_eq!(h.clip_count(), 2);
    assert_eq!(h.cap().skip_counters().storage_full(), 0);
}

#[test]
fn storage_full_twice_reports_unavailable() {
    let h = start(PrivacyPolicy::default());
    h.sentinel();
    let hooks = &h.cap().hooks;
    let before = hooks.attempts.load(Ordering::SeqCst);
    hooks
        .inject
        .lock()
        .unwrap()
        .extend([StorageError::StorageFull, StorageError::StorageFull]);
    h.fake.copy(text_copy("never stored"));
    let (_, seen) = h.wait_for("Unavailable", |e| {
        *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
    });
    assert!(no_store_events(&seen), "{seen:?}");
    // Exactly one retention run, one retry.
    assert_eq!(hooks.attempts.load(Ordering::SeqCst), before + 1);
    assert_eq!(h.cap().skip_counters().storage_full(), 1);
    assert_eq!(h.clip_count(), 1);
    // The actor is alive and stores again.
    h.fake.copy(text_copy("after the failure"));
    h.wait_added();
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn storage_full_then_other_retry_error_is_counted() {
    let h = start(PrivacyPolicy::default());
    h.sentinel();
    let hooks = &h.cap().hooks;
    hooks.inject.lock().unwrap().extend([
        StorageError::StorageFull,
        StorageError::Sqlite("Other".into()),
    ]);
    h.fake.copy(text_copy("never stored either"));
    let (_, seen) = h.wait_for("Unavailable", |e| {
        *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
    });
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.cap().skip_counters().storage_full(), 1);
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn non_full_insert_error_is_not_retried() {
    let h = start(PrivacyPolicy::default());
    h.sentinel();
    let hooks = &h.cap().hooks;
    let before = hooks.attempts.load(Ordering::SeqCst);
    hooks
        .inject
        .lock()
        .unwrap()
        .push_back(StorageError::Sqlite("Other".into()));
    h.fake.copy(text_copy("single failure"));
    h.wait_for("Unavailable", |e| {
        *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
    });
    assert_eq!(hooks.attempts.load(Ordering::SeqCst), before);
    assert_eq!(h.cap().skip_counters().storage_full(), 0);
}

#[test]
fn html_only_api_key_dropped() {
    let h = start(PrivacyPolicy::default());
    let html = format!("<p>{}</p>", api_key_text());
    h.fake.copy(FakeCopy {
        formats: vec![(
            pf("HTML Format", Some(Mime::TextHtml), html.len()),
            html.into_bytes(),
        )],
        markers: Vec::new(),
        source: None,
    });
    let seen = h.wait_skip(SkipReason::Sensitive);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 1);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn seq_change_during_read_drops_bytes() {
    let h = start(PrivacyPolicy::default());
    h.fake.bump_seq_on_next_read();
    h.fake.copy(text_copy("raced text"));
    let deadline = Instant::now() + WAIT;
    while h.cap().skip_counters().changed_during_read() == 0 {
        assert!(Instant::now() < deadline, "no changed-during-read count");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(h.fake.read_calls(), 1);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    let ro = h.ro();
    let (rows, _) = ro
        .search(&SearchQuery {
            text: Some("raced".into()),
            kinds: Vec::new(),
            pinned_only: false,
            since: None,
            include_masked: true,
            limit: 10,
            cursor: None,
            collection: None,
            tag: None,
        })
        .unwrap();
    assert!(rows.is_empty());
    assert_eq!(h.cap().skip_counters().changed_during_read(), 1);
}

#[test]
fn kind_disabled_after_classification_dropped() {
    let policy = PrivacyPolicy {
        capture_kinds: vec![ClipType::Url],
        ..PrivacyPolicy::default()
    };
    let h = start(policy);
    h.fake.copy(text_copy("just some ordinary prose"));
    let seen = h.wait_skip(SkipReason::KindDisabled);
    assert!(no_store_events(&seen), "{seen:?}");
    // Text/plain can become a URL, so the policy allowed the read; the drop is post-read.
    assert_eq!(h.fake.read_calls(), 1);
    assert_eq!(h.clip_count(), 0);
    h.fake.copy(text_copy("https://example.com/page"));
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().kind, ClipType::Url);
}

#[test]
fn events_and_errors_carry_no_content() {
    let id = ClipId::new_v7(Timestamp(T0));
    let dbg = format!(
        "{:?} {:?} {:?}",
        AppEvent::ClipAdded(id),
        AppEvent::Skipped(SkipReason::Sensitive),
        ApiError::InvalidArgument("clip has no plain text")
    );
    assert!(dbg.contains("ClipAdded") && dbg.contains("Sensitive"));
}
