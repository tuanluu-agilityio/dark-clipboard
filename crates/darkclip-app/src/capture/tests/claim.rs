use super::*;

// Task 603: the capture claim for translate in place. The source copy is handed to the
// requester and never stored; every privacy skip still wins.

pub(super) type ClaimRx = Receiver<Result<ClaimedText, ClaimError>>;

/// Arms the claim slot for `app_id`.
pub(super) fn arm(h: &H, app_id: &str) -> ClaimRx {
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::ExpectForeignCopy {
            app_id: app_id.to_string(),
            reply: tx,
        })
        .unwrap();
    rx
}

/// A window long enough that a slow machine never expires a claim the test expects to
/// succeed (F-16 / F-19). Read by the actor when the slot is armed.
pub(super) fn long_window(h: &H) {
    h.cap()
        .hooks
        .claim_window_ms
        .store(30_000, Ordering::SeqCst);
}

pub(super) fn claim_reply(rx: &ClaimRx) -> Result<String, ClaimError> {
    rx.recv_timeout(WAIT).unwrap().map(|r| r.text.0)
}

pub(super) fn owned(app: &str, text: &str) -> FakeCopy {
    FakeCopy {
        source: Some(src(app)),
        ..text_copy(text)
    }
}

fn restore_candidate(h: &H) -> Option<ClipId> {
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::RestoreCandidate { reply: tx })
        .unwrap();
    rx.recv_timeout(WAIT).unwrap()
}

pub(super) fn copy_text(h: &H, text: &str) -> Result<u64, ApiError> {
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::CopyText {
            text: darkclip_core::Redacted(text.to_string()),
            deadline: Instant::now() + WAIT,
            reply: tx,
        })
        .unwrap();
    rx.recv_timeout(WAIT).unwrap()
}

fn ole_cfg() -> CaptureConfig {
    CaptureConfig {
        debounce_ms: 1_000,
        ..test_cfg()
    }
}

pub(super) fn ole_owned(app: &str, text: &str) -> FakeCopy {
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

pub(super) fn ole_flush(text: &str) -> FakeCopy {
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

fn paused_policy() -> PrivacyPolicy {
    PrivacyPolicy {
        mode: CaptureMode::Paused {
            until: Timestamp(T0 + 3_600_000),
        },
        ..PrivacyPolicy::default()
    }
}

fn disabled_policy() -> PrivacyPolicy {
    PrivacyPolicy {
        capture_enabled: false,
        ..PrivacyPolicy::default()
    }
}

fn no_skip_events(events: &[AppEvent]) -> bool {
    !events.iter().any(|e| matches!(e, AppEvent::Skipped(_)))
}

#[test]
fn claimed_change_is_returned_and_never_stored() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("zalo.exe", "xin chao"));
    assert_eq!(claim_reply(&rx), Ok("xin chao".to_string()));
    assert_eq!(h.fake.read_calls(), 1);
    assert_eq!(h.fake.snapshot_meta_calls(), 1);
    // The seq is processed: more signals for it are not looked at again.
    h.fake.signal_again(2);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert!(no_skip_events(&between), "{between:?}");
    assert_eq!(h.fake.snapshot_meta_calls(), 2);
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "xin chao"));
    // Windows exe names match without case; the slot is single use.
    let rx = arm(&h, "Zalo.EXE");
    h.fake
        .copy_burst_at(vec![(Timestamp(T0), owned("zalo.exe", "lan hai"))]);
    assert_eq!(claim_reply(&rx), Ok("lan hai".to_string()));
    // Beyond the same-app follow-up skip (Task 603b: 1500 ms by signal time).
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + 2_000),
        owned("zalo.exe", "stored again"),
    )]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(
        h.ro().get_clip(&id).unwrap().unwrap().preview,
        "stored again"
    );
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn other_app_change_is_stored_and_slot_stays_armed_until_expiry() {
    // Still armed after another app's change: the target's next change is claimed.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("chrome.exe", "other app text"));
    let (id, _) = h.wait_added();
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "other app text");
    assert_eq!(clip.source.map(|s| s.app_id).as_deref(), Some("chrome.exe"));
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    h.fake.copy(owned("zalo.exe", "target text"));
    assert_eq!(claim_reply(&rx), Ok("target text".to_string()));
    assert_eq!(h.clip_count(), 1);

    // With the real window: the other app's change is stored, then the slot expires.
    let h = start(PrivacyPolicy::default());
    let t = Instant::now();
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("chrome.exe", "other app text"));
    h.wait_added();
    assert_eq!(claim_reply(&rx), Err(ClaimError::Expired));
    assert!(t.elapsed() >= Duration::from_millis(CLAIM_WINDOW_MS));
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn ownerless_change_without_target_heir_is_not_claimed() {
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    long_window(&h);
    let rx = arm(&h, "notepad.exe");
    // No owner at all.
    h.fake.copy(text_copy("nobody's text"));
    let (id, _) = h.wait_added();
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, "nobody's text");
    assert!(clip.source.is_none());
    // The OLE flush shape, but the heir is another app.
    h.fake.copy_burst(vec![
        ole_owned("chrome.exe", "chrome text"),
        ole_flush("chrome text"),
    ]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.source.map(|s| s.app_id).as_deref(), Some("chrome.exe"));
    // An ownerless flush with no owned phase before it.
    h.fake.copy(ole_flush("lone flush"));
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "lone flush");
    // Never claimed: the slot is still armed for the target.
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    h.fake.copy(owned("notepad.exe", "target text"));
    assert_eq!(claim_reply(&rx), Ok("target text".to_string()));
    assert_eq!(h.clip_count(), 3);
}

#[test]
fn ownerless_ole_flush_with_target_heir_is_claimed() {
    // Notepad shape (F-14): an owned phase, then the ownerless flush, in one burst. While
    // paused, the flush gets past step 1 because the pending change it follows is the
    // target's.
    for paused in [false, true] {
        let policy = if paused {
            paused_policy()
        } else {
            PrivacyPolicy::default()
        };
        let h = start_with(policy, RetentionPolicy::default(), ole_cfg(), |_| {});
        long_window(&h);
        let rx = arm(&h, "notepad.exe");
        h.fake.copy_burst(vec![
            ole_owned("notepad.exe", "flushed text"),
            ole_flush("flushed text"),
        ]);
        assert_eq!(
            claim_reply(&rx),
            Ok("flushed text".to_string()),
            "paused={paused}"
        );
        assert_eq!(h.fake.snapshot_meta_calls(), 1, "paused={paused}");
        if !paused {
            let between = h.sentinel();
            assert!(no_store_events(&between), "{between:?}");
            assert_eq!(h.clip_count(), 1);
        }
        assert!(!h.stored_previews().iter().any(|p| p == "flushed text"));
    }
}

#[test]
fn late_ole_flush_of_claimed_copy_is_not_stored() {
    // Notepad shape across two bursts: the owned phase is claimed, then its ownerless flush
    // arrives (20 ms later by signal time). It must not be stored as a clip.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "notepad.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0),
        ole_owned("notepad.exe", "source text"),
    )]);
    assert_eq!(claim_reply(&rx), Ok("source text".to_string()));
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 20), ole_flush("source text"))]);
    let metas = h.fake.snapshot_meta_calls();
    let deadline = Instant::now() + WAIT;
    while h.fake.snapshot_meta_calls() == metas {
        assert!(Instant::now() < deadline, "the flush was never looked at");
        std::thread::sleep(Duration::from_millis(1));
    }
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.fake.read_calls(), 2);
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "source text"));
    // Beyond the burst cap by signal time it is an ordinary ownerless change: stored.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "notepad.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0),
        ole_owned("notepad.exe", "source text"),
    )]);
    assert_eq!(claim_reply(&rx), Ok("source text".to_string()));
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 600), ole_flush("later text"))]);
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "later text");
}

/// Waits until the actor has dropped the fake's own signals listed in `drop_signal`.
fn wait_signal_lost(h: &H) {
    let deadline = Instant::now() + WAIT;
    while !h.cap().hooks.drop_signal.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the signal never arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn let_through_change_is_never_stored_after_resume() {
    // Paused until T0 + 60 s; the debounce keeps the change pending while capture resumes.
    let policy = PrivacyPolicy {
        mode: CaptureMode::Paused {
            until: Timestamp(T0 + 60_000),
        },
        ..PrivacyPolicy::default()
    };
    // 1. The pause ends by its deadline: the target's signal was let through, but the
    //    snapshot is a newer change of another app (never claimed).
    let h = start_with(
        policy.clone(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    // The fake's own signal is replaced by one sent in the actor's queue, so a later command
    // proves the actor has let it through before the clock moves.
    h.cap().hooks.drop_signal.lock().unwrap().push(1);
    let newer = h.fake.copy_then_replace_silently(
        owned("zalo.exe", "target text"),
        owned("chrome.exe", "copied during the pause"),
    );
    assert_eq!(newer, 2);
    wait_signal_lost(&h);
    h.cap()
        .tx
        .as_ref()
        .unwrap()
        .send(Msg::Signal(ChangeSignal {
            seq: 1,
            at: Timestamp(T0),
            source: Some(src("zalo.exe")),
        }))
        .unwrap();
    assert_eq!(claim_reply(&arm(&h, "other.exe")), Err(ClaimError::Busy));
    h.clock.set(T0 + 120_000);
    let seen = h.wait_skip(SkipReason::Paused);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert!(!h
        .stored_previews()
        .iter()
        .any(|p| p == "copied during the pause"));
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

    // 2. The user resumes (SetMode) while the target's change is pending: never stored.
    let h = start_with(policy, RetentionPolicy::default(), ole_cfg(), |_| {});
    long_window(&h);
    let _rx = arm(&h, "zalo.exe");
    h.cap().hooks.drop_signal.lock().unwrap().push(1);
    h.fake.copy(owned("zalo.exe", "target text"));
    wait_signal_lost(&h);
    h.cap()
        .tx
        .as_ref()
        .unwrap()
        .send(Msg::Signal(ChangeSignal {
            seq: 1,
            at: Timestamp(T0),
            source: Some(src("zalo.exe")),
        }))
        .unwrap();
    assert_eq!(claim_reply(&arm(&h, "other.exe")), Err(ClaimError::Busy));
    h.set_mode(CaptureMode::Normal);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "target text"));
}

#[test]
fn claim_with_gone_requester_is_not_stored() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    drop(arm(&h, "zalo.exe"));
    h.fake.copy(owned("zalo.exe", "nobody waits"));
    // The claim was read (and its reply could not be delivered) before the sentinel.
    let deadline = Instant::now() + WAIT;
    while h.fake.read_calls() == 0 {
        assert!(Instant::now() < deadline, "the claim was never read");
        std::thread::sleep(Duration::from_millis(1));
    }
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.fake.read_calls(), 2);
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "nobody waits"));
}

#[test]
fn claim_works_while_paused_and_while_disabled_and_nothing_is_stored() {
    for (policy, reason) in [
        (paused_policy(), SkipReason::Paused),
        (disabled_policy(), SkipReason::Disabled),
    ] {
        let h = start(policy);
        long_window(&h);
        let rx = arm(&h, "zalo.exe");
        // Another app's change, or an ownerless one, is dropped at step 1 as without a claim:
        // not even its metadata is looked at (privacy-design section 5).
        h.fake.copy(owned("chrome.exe", "other app text"));
        let seen = h.wait_skip(reason);
        assert!(no_store_events(&seen), "{seen:?}");
        h.fake.copy(text_copy("nobody's text"));
        let seen = h.wait_skip(reason);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        // The target's change is claimed.
        h.fake.copy(owned("zalo.exe", "target text"));
        assert_eq!(claim_reply(&rx), Ok("target text".to_string()));
        assert_eq!(h.fake.read_calls(), 1);
        // Disarmed: the gate is as before (no clipboard access at all).
        let metas = h.fake.snapshot_meta_calls();
        h.fake.copy(owned("zalo.exe", "after the claim"));
        let seen = h.wait_skip(reason);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.snapshot_meta_calls(), metas);
        assert_eq!(h.fake.read_calls(), 1);
        assert_eq!(h.clip_count(), 0);
    }
}

#[test]
fn claim_refused_in_private_mode() {
    for policy in [
        PrivacyPolicy {
            mode: CaptureMode::Private,
            ..PrivacyPolicy::default()
        },
        // Disabled is the first gate reason, but Private still wins over the claim.
        PrivacyPolicy {
            mode: CaptureMode::Private,
            capture_enabled: false,
            ..PrivacyPolicy::default()
        },
    ] {
        let h = start(policy);
        long_window(&h);
        let rx = arm(&h, "zalo.exe");
        h.fake.copy(owned("zalo.exe", "private text"));
        assert_eq!(
            claim_reply(&rx),
            Err(ClaimError::Skipped(SkipReason::Private))
        );
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        assert_eq!(h.clip_count(), 0);
    }
}

#[test]
fn claim_respects_markers_excluded_app_and_transient() {
    let marked = |markers: Vec<PrivacyMarker>| FakeCopy {
        markers,
        ..owned("zalo.exe", "marked text")
    };
    let cases: Vec<(&str, FakeCopy, SkipReason)> = vec![
        (
            "zalo.exe",
            marked(vec![PrivacyMarker::OsConcealed {
                name: "CanIncludeInClipboardHistory",
            }]),
            SkipReason::OsConcealed,
        ),
        (
            "zalo.exe",
            marked(vec![PrivacyMarker::DarkshotExcluded]),
            SkipReason::DarkshotExcluded,
        ),
        (
            "zalo.exe",
            marked(vec![PrivacyMarker::Transient]),
            SkipReason::Transient,
        ),
        (
            "zalo.exe",
            marked(vec![PrivacyMarker::AutoGenerated]),
            SkipReason::Transient,
        ),
        (
            "KeePassXC.exe",
            excluded_copy("vault entry"),
            SkipReason::ExcludedApp,
        ),
    ];
    for (paused, policy) in [(false, PrivacyPolicy::default()), (true, paused_policy())] {
        for (target, copy, reason) in cases.iter().map(|(t, c, r)| {
            (
                *t,
                FakeCopy {
                    formats: c.formats.clone(),
                    markers: c.markers.clone(),
                    source: c.source.clone(),
                },
                *r,
            )
        }) {
            let h = start(policy.clone());
            long_window(&h);
            let rx = arm(&h, target);
            h.fake.copy(copy);
            assert_eq!(
                claim_reply(&rx),
                Err(ClaimError::Skipped(reason)),
                "{reason:?} paused={paused}"
            );
            // Counted and reported as today, nothing read or stored.
            h.wait_skip(reason);
            assert_eq!(h.fake.read_calls(), 0, "{reason:?}");
            assert_eq!(h.cap().skip_counters().get(reason), 1, "{reason:?}");
            assert_eq!(h.clip_count(), 0, "{reason:?}");
        }
    }
    // Another source seen for the same change is excluded: the whole change is skipped.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake
        .copy_then_set_source(owned("zalo.exe", "vault entry"), Some(src("KeePassXC.exe")));
    assert_eq!(
        claim_reply(&rx),
        Err(ClaimError::Skipped(SkipReason::ExcludedApp))
    );
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn claim_refuses_no_cloud_marker() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(FakeCopy {
        markers: vec![PrivacyMarker::NoCloud],
        ..owned("zalo.exe", "stay on this device")
    });
    assert_eq!(claim_reply(&rx), Err(ClaimError::NoCloud));
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn claim_expires_without_activity() {
    let h = start(PrivacyPolicy::default());
    let t = Instant::now();
    let rx = arm(&h, "zalo.exe");
    assert_eq!(claim_reply(&rx), Err(ClaimError::Expired));
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(CLAIM_WINDOW_MS), "{took:?}");
    assert!(
        took < Duration::from_millis(CLAIM_WINDOW_MS + 500),
        "{took:?}"
    );
    // Disarmed: a later change from the target is stored as usual.
    h.fake.copy(owned("zalo.exe", "later text"));
    h.wait_added();
    assert_eq!(h.fake.snapshot_meta_calls(), 1);
}

#[test]
fn second_claim_while_armed_is_busy() {
    let h = start(PrivacyPolicy::default());
    let first = arm(&h, "zalo.exe");
    let second = arm(&h, "slack.exe");
    assert_eq!(claim_reply(&second), Err(ClaimError::Busy));
    // The first slot stays (it expires on its own).
    assert_eq!(claim_reply(&first), Err(ClaimError::Expired));
    // A new slot after expiry is armed normally.
    long_window(&h);
    let third = arm(&h, "slack.exe");
    h.fake.copy(owned("slack.exe", "third text"));
    assert_eq!(claim_reply(&third), Ok("third text".to_string()));
}

#[test]
fn claim_over_cap_is_too_long_and_not_stored() {
    let retention = RetentionPolicy {
        text_max_kb: 1,
        ..RetentionPolicy::default()
    };
    let h = start_with(PrivacyPolicy::default(), retention, test_cfg(), |_| {});
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("zalo.exe", &"a".repeat(2_000)));
    assert_eq!(claim_reply(&rx), Err(ClaimError::TooLong));
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    // At the cap exactly: returned whole.
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("zalo.exe", &"b".repeat(1_024)));
    assert_eq!(claim_reply(&rx), Ok("b".repeat(1_024)));
}

#[test]
fn claim_without_text_is_not_text() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(FakeCopy {
        formats: vec![(
            pf("HTML Format", Some(Mime::TextHtml), 20),
            b"<b>bold</b>".to_vec(),
        )],
        markers: Vec::new(),
        source: Some(src("zalo.exe")),
    });
    assert_eq!(claim_reply(&rx), Err(ClaimError::NotText));
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn change_signalled_before_arming_is_not_claimed() {
    // The change is pending (long debounce) before the slot exists: stored as today.
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    long_window(&h);
    // The signal carries no owner, so the actor resolves it when it dequeues the signal:
    // `resolve_calls` tells the test the change is pending.
    h.fake
        .copy_then_set_source(text_copy("copied before"), Some(src("zalo.exe")));
    let deadline = Instant::now() + WAIT;
    while h.fake.resolve_calls() == 0 {
        assert!(Instant::now() < deadline, "signal never dequeued");
        std::thread::sleep(Duration::from_millis(1));
    }
    let rx = arm(&h, "zalo.exe");
    let (id, _) = h.wait_added();
    assert_eq!(
        h.ro().get_clip(&id).unwrap().unwrap().preview,
        "copied before"
    );
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn gate_unchanged_when_no_slot_armed() {
    for (policy, reason) in [
        (paused_policy(), SkipReason::Paused),
        (disabled_policy(), SkipReason::Disabled),
    ] {
        let h = start(policy);
        h.fake.copy(owned("zalo.exe", "not stored"));
        let seen = h.wait_skip(reason);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        // After a slot expired: the same.
        let rx = arm(&h, "zalo.exe");
        assert_eq!(claim_reply(&rx), Err(ClaimError::Expired));
        h.fake.copy(owned("zalo.exe", "still not stored"));
        h.wait_skip(reason);
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        assert_eq!(h.clip_count(), 0);
    }
}

#[test]
fn restore_candidate_some_only_for_latest_stored_seq() {
    let h = start(PrivacyPolicy::default());
    assert_eq!(restore_candidate(&h), None);
    // Stored.
    h.fake.copy(text_copy("first clip"));
    let (id, _) = h.wait_added();
    assert_eq!(restore_candidate(&h), Some(id));
    // Deduplicated: the same clip, for the newer seq.
    h.fake.copy(text_copy("first clip"));
    h.wait_for("ClipBumped", |e| *e == AppEvent::ClipBumped(id));
    assert_eq!(restore_candidate(&h), Some(id));
    // Skipped.
    h.fake.copy(excluded_copy("vault entry"));
    h.wait_skip(SkipReason::ExcludedApp);
    assert_eq!(restore_candidate(&h), None);
    // Stored again, then claimed.
    // Owned: an ownerless copy this soon after an excluded skip is skipped (F-14 rule 3).
    h.fake.copy(owned("notepad.exe", "second clip"));
    let (id2, _) = h.wait_added();
    assert_eq!(restore_candidate(&h), Some(id2));
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("zalo.exe", "source text"));
    assert_eq!(claim_reply(&rx), Ok("source text".to_string()));
    assert_eq!(restore_candidate(&h), None);
    // Own write (processed as a self-write skip).
    h.fake.copy(owned("notepad.exe", "third clip"));
    let (id3, _) = h.wait_added();
    assert_eq!(restore_candidate(&h), Some(id3));
    copy_text(&h, "translated text").unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    assert_eq!(restore_candidate(&h), None);

    // A newer change not processed yet (long debounce keeps it pending).
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        ole_cfg(),
        |_| {},
    );
    h.fake.copy(text_copy("stored clip"));
    let (id, _) = h.wait_added();
    assert_eq!(restore_candidate(&h), Some(id));
    h.fake.copy(text_copy("newer, not processed"));
    assert_eq!(restore_candidate(&h), None);
}

#[test]
fn copy_text_returns_own_seq() {
    let h = start(PrivacyPolicy::default());
    let seq = h.fake.copy(text_copy("before"));
    h.wait_added();
    assert_eq!(copy_text(&h, "own text"), Ok(seq + 1));
    assert_eq!(copy_text(&h, "own text 2"), Ok(seq + 2));
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 2);
    assert_eq!(
        copy_text(&h, ""),
        Err(ApiError::InvalidArgument("nothing to copy"))
    );
}

#[test]
fn two_own_writes_in_a_row_are_never_stored() {
    #[derive(Clone, Copy, Debug)]
    enum Order {
        AB,
        BA,
        BOnly,
    }
    for order in [Order::AB, Order::BA, Order::BOnly] {
        let h = start(PrivacyPolicy::default());
        let before = h.fake.copy(text_copy("restored clip"));
        let (id, _) = h.wait_added();
        let a = before + 1;
        if matches!(order, Order::BA | Order::BOnly) {
            // A's own signal is lost (or late): the actor never sees it now.
            h.cap().hooks.drop_signal.lock().unwrap().push(a);
        }
        // A: the translation; B: restoring the stored clip. Both sent before either reply
        // so the signals can arrive after both writes.
        let (ta, ra) = mpsc::channel();
        let (tb, rb) = mpsc::channel();
        h.cap()
            .send(ActorCommand::CopyText {
                text: darkclip_core::Redacted("translation".to_string()),
                deadline: Instant::now() + WAIT,
                reply: ta,
            })
            .unwrap();
        h.cap()
            .send(ActorCommand::CopyBack {
                id,
                plain: false,
                deadline: Instant::now() + WAIT,
                reply: tb,
            })
            .unwrap();
        assert_eq!(ra.recv_timeout(WAIT).unwrap(), Ok(a), "{order:?}");
        assert_eq!(rb.recv_timeout(WAIT).unwrap(), Ok(()), "{order:?}");
        if matches!(order, Order::BA | Order::BOnly) {
            // B's signal is dropped as our own write.
            h.wait_skip(SkipReason::SelfWrite);
        }
        if matches!(order, Order::BA) {
            // A's signal arrives after B's, and the actor looks at the clipboard for it.
            wait_signal_lost(&h);
            let metas = h.fake.snapshot_meta_calls();
            h.cap()
                .tx
                .as_ref()
                .unwrap()
                .send(Msg::Signal(ChangeSignal {
                    seq: a,
                    at: Timestamp(T0),
                    source: None,
                }))
                .unwrap();
            let deadline = Instant::now() + WAIT;
            while h.fake.snapshot_meta_calls() == metas {
                assert!(Instant::now() < deadline, "A's signal was never looked at");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let between = h.sentinel();
        assert!(no_store_events(&between), "{order:?} {between:?}");
        assert_eq!(h.clip_count(), 2, "{order:?}");
        assert!(
            !h.stored_previews().iter().any(|p| p == "translation"),
            "{order:?}"
        );
        assert_eq!(h.fake.read_calls(), 2, "{order:?}");
    }
}

#[test]
fn claim_changed_during_read_is_discarded() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    h.fake.bump_seq_on_next_read();
    h.fake.copy(owned("zalo.exe", "source text"));
    assert_eq!(claim_reply(&rx), Err(ClaimError::Skipped(SkipReason::Busy)));
    assert_eq!(h.cap().skip_counters().changed_during_read(), 1);
    assert_eq!(h.clip_count(), 0);
}
