use super::*;

#[test]
fn masked_clip_removed_at_expiry_not_ten_minutes_later() {
    let h = start(masked_policy());
    h.fake.copy(text_copy(&api_key_text()));
    let (id, _) = h.wait_added();
    let ttl = i64::from(RetentionPolicy::default().sensitive_ttl_seconds);
    h.clock.set(T0 + ttl * 1000);
    h.sentinel();
    assert!(h.ro().get_clip(&id).unwrap().is_none());
    assert_eq!(h.clip_count(), 1);
}

fn seed_masked(expires_at: i64) -> impl FnOnce(&mut Storage) {
    move |s: &mut Storage| {
        let reps = vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: b"masked seed".to_vec(),
            width: None,
            height: None,
            truncated: false,
        }];
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        s.upsert_clip(&NewClip {
            classified,
            hash,
            reps,
            source: None,
            sensitivity: Sensitivity {
                level: SensitivityLevel::Masked,
                categories: vec![SensitiveCategory::ApiKey],
            },
            expires_at: Some(Timestamp(expires_at)),
            thumbnail_png: None,
            no_export: false,
        })
        .unwrap();
    }
}

#[test]
fn retention_recovery_emits_retention_ok() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        test_cfg(),
        |_| {},
    );
    let hooks = &h.cap().hooks;
    hooks.fail.store(true, Ordering::SeqCst);
    let send_policy = || {
        h.cap()
            .send(ActorCommand::UpdatePolicy(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
            ))
            .unwrap();
    };
    send_policy();
    h.wait_for("retention failed", |e| {
        *e == AppEvent::Unavailable(UNAVAILABLE_RETENTION)
    });
    hooks.fail.store(false, Ordering::SeqCst);
    send_policy();
    h.wait_for("RetentionOk", |e| *e == AppEvent::RetentionOk);
}

#[test]
fn expiry_lookup_failure_falls_back_to_timer() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        test_cfg(),
        seed_masked(T0 + 1_000),
    );
    h.sentinel();
    let hooks = &h.cap().hooks;
    hooks.expiry_fail.store(true, Ordering::SeqCst);
    // A retention run that succeeds but cannot read the next expiry: the schedule is dropped.
    h.cap()
        .send(ActorCommand::UpdatePolicy(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
        ))
        .unwrap();
    assert_eq!(
        h.copy_back(ClipId::new_v7(Timestamp(T0)), false),
        Err(ApiError::NotFound)
    );
    let before = hooks.attempts.load(Ordering::SeqCst);
    h.clock.set(T0 + 2_000);
    h.fake.signal_again(1);
    std::thread::sleep(Duration::from_millis(200));
    // No expiry-driven retention (the 10-minute timer is far away), so the clip stays.
    assert_eq!(hooks.attempts.load(Ordering::SeqCst), before);
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn failed_retention_backs_off() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        test_cfg(),
        seed_masked(T0 + 1_000),
    );
    h.sentinel();
    let hooks = &h.cap().hooks;
    hooks.fail.store(true, Ordering::SeqCst);
    let before = hooks.attempts.load(Ordering::SeqCst);
    // Past expiry, and every retention attempt fails.
    h.clock.set(T0 + 2_000);
    h.fake.signal_again(1);
    std::thread::sleep(Duration::from_millis(200));
    let attempts = hooks.attempts.load(Ordering::SeqCst) - before;
    let failures = h
        .events
        .try_iter()
        .filter(|e| *e == AppEvent::Unavailable("retention failed"))
        .count();
    assert!((1..=2).contains(&attempts), "attempts {attempts}");
    assert!((1..=2).contains(&failures), "Unavailable events {failures}");
    // Nothing was deleted while failing.
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn retention_backoff_doubles_to_ten_minutes() {
    assert_eq!(next_backoff(None), Duration::from_secs(30));
    assert_eq!(
        next_backoff(Some(Duration::from_secs(30))),
        Duration::from_secs(60)
    );
    assert_eq!(
        next_backoff(Some(Duration::from_secs(480))),
        Duration::from_secs(600)
    );
    assert_eq!(
        next_backoff(Some(Duration::from_secs(600))),
        Duration::from_secs(600)
    );
}

#[test]
fn masked_clip_from_earlier_session_removed_at_expiry() {
    let seed = |s: &mut Storage| {
        let reps = vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: b"masked from last run".to_vec(),
            width: None,
            height: None,
            truncated: false,
        }];
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        s.upsert_clip(&NewClip {
            classified,
            hash,
            reps,
            source: None,
            sensitivity: Sensitivity {
                level: SensitivityLevel::Masked,
                categories: vec![SensitiveCategory::ApiKey],
            },
            expires_at: Some(Timestamp(T0 + 5_000)),
            thumbnail_png: None,
            no_export: false,
        })
        .unwrap();
    };
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        test_cfg(),
        seed,
    );
    // Not expired yet: survives the start-up retention.
    h.sentinel();
    assert_eq!(h.clip_count(), 2);
    h.clock.set(T0 + 5_000);
    h.fake.copy(text_copy("second sentinel"));
    h.wait_added();
    assert_eq!(h.clip_count(), 2);
    let ro = h.ro();
    let (rows, _) = ro
        .search(&SearchQuery {
            text: None,
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
    assert!(rows.iter().all(|r| r.sensitivity == SensitivityLevel::None));
}

#[test]
fn retention_after_insert_over_limit() {
    let retention = RetentionPolicy {
        max_items: 2,
        ..RetentionPolicy::default()
    };
    let h = start_with(PrivacyPolicy::default(), retention, test_cfg(), |_| {});
    for s in ["one", "two", "three"] {
        h.fake.copy(text_copy(s));
        h.wait_added();
    }
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn retention_runs_on_start() {
    let seed = |s: &mut Storage| {
        let reps = vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: b"expired masked".to_vec(),
            width: None,
            height: None,
            truncated: false,
        }];
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        s.upsert_clip(&NewClip {
            classified,
            hash,
            reps,
            source: None,
            sensitivity: Sensitivity {
                level: SensitivityLevel::Masked,
                categories: vec![SensitiveCategory::ApiKey],
            },
            expires_at: Some(Timestamp(T0 - 1)),
            thumbnail_png: None,
            no_export: false,
        })
        .unwrap();
    };
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        test_cfg(),
        seed,
    );
    h.sentinel();
    assert_eq!(h.clip_count(), 1);
}

/// A word that is unique in the file and far from secret-shaped (GC-09).
const TOKEN: &str = "qzvkjxwunique";

fn file_has_token(h: &H) -> bool {
    let mut all = std::fs::read(h.dir.path().join("history.db")).unwrap();
    if let Ok(wal) = std::fs::read(h.dir.path().join("history.db-wal")) {
        all.extend(wal);
    }
    all.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes())
}

fn copy_at(h: &H, ms: i64, text: &str) {
    h.clock.set(T0 + ms);
    h.fake.copy(text_copy(text));
    h.wait_added();
}

fn two_item_limit() -> RetentionPolicy {
    RetentionPolicy {
        max_items: 2,
        max_age_days: 0,
        max_storage_mb: 0,
        ..RetentionPolicy::default()
    }
}

/// B1 follow-up: the merge deadline starts at the first pending eviction and later captures
/// (each of which re-arms the retention interval) must not push it back.
#[test]
fn evicted_words_leave_the_file_ten_minutes_after_the_first_eviction_despite_steady_captures() {
    const MIN: i64 = 60_000;
    let h = start_with(
        PrivacyPolicy::default(),
        two_item_limit(),
        test_cfg(),
        |_| {},
    );
    copy_at(&h, 0, &format!("oldest {TOKEN}"));
    copy_at(&h, 1_000, "second clip");
    // The third copy evicts the oldest one: the merge is deferred, the word is still there.
    copy_at(&h, 2_000, "third clip");
    assert_eq!(h.clip_count(), 2);
    assert!(file_has_token(&h), "automatic eviction defers the merge");
    // A copy at least every 10 minutes keeps the retention interval from ever elapsing.
    copy_at(&h, 4 * MIN, "fourth clip");
    copy_at(&h, 8 * MIN, "fifth clip");
    copy_at(&h, 9 * MIN + 30_000, "sixth clip");
    assert!(
        file_has_token(&h),
        "not yet 10 minutes after the first eviction"
    );
    copy_at(&h, 10 * MIN + 5_000, "seventh clip");
    assert!(
        !file_has_token(&h),
        "10 minutes after the first pending eviction the index was merged"
    );
}

/// The retention tick merges what automatic evictions left pending.
#[test]
fn the_retention_tick_merges_pending_evictions() {
    let h = start_with(
        PrivacyPolicy::default(),
        two_item_limit(),
        test_cfg(),
        |_| {},
    );
    copy_at(&h, 0, &format!("oldest {TOKEN}"));
    copy_at(&h, 1_000, "second clip");
    copy_at(&h, 2_000, "third clip");
    assert!(file_has_token(&h));
    // A short interval from now on; a policy update runs retention, which re-arms it.
    h.cap().hooks.tick_ms.store(50, Ordering::SeqCst);
    h.cap()
        .send(ActorCommand::UpdatePolicy(
            PrivacyPolicy::default(),
            two_item_limit(),
        ))
        .unwrap();
    let until = Instant::now() + WAIT;
    while file_has_token(&h) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!file_has_token(&h), "the tick merged the pending index");
}
