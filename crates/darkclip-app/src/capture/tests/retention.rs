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
