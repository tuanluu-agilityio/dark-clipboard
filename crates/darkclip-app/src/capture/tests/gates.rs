use super::*;

#[test]
fn excluded_never_read() {
    let h = start(PrivacyPolicy::default());
    let markers = [
        PrivacyMarker::OsConcealed {
            name: "ExcludeClipboardContentFromMonitorProcessing",
        },
        PrivacyMarker::OsConcealed {
            name: "CanIncludeInClipboardHistory",
        },
        PrivacyMarker::OsConcealed {
            name: "org.nspasteboard.ConcealedType",
        },
        PrivacyMarker::DarkshotExcluded,
        PrivacyMarker::PasswordManagerHint,
    ];
    for m in markers {
        assert!(is_mandatory(&m));
        let mut c = text_copy("hunter2 is my password");
        c.markers = vec![PrivacyMarker::NoCloud, m];
        h.fake.copy(c);
        let (ev, seen) = h.wait_for("Skipped", |e| matches!(e, AppEvent::Skipped(_)));
        assert!(
            matches!(
                ev,
                AppEvent::Skipped(SkipReason::OsConcealed | SkipReason::DarkshotExcluded)
            ),
            "{m:?} -> {ev:?}"
        );
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0, "{m:?}");
    }
    assert_eq!(h.clip_count(), 0);
    let c = h.cap().skip_counters();
    assert_eq!(
        c.get(SkipReason::OsConcealed) + c.get(SkipReason::DarkshotExcluded),
        5
    );
}

#[test]
fn paused_never_reads_formats() {
    let h = start(PrivacyPolicy::default());
    h.set_mode(CaptureMode::Paused {
        until: Timestamp(T0 + 60_000),
    });
    h.fake.copy(text_copy("copied while paused"));
    h.wait_skip(SkipReason::Paused);
    assert_eq!(h.fake.snapshot_meta_calls(), 0);
    assert_eq!(h.fake.read_calls(), 0);
    h.set_mode(CaptureMode::Private);
    h.fake.copy(text_copy("copied in private mode"));
    h.wait_skip(SkipReason::Private);
    assert_eq!(h.fake.snapshot_meta_calls(), 0);
    assert_eq!(h.fake.read_calls(), 0);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn resume_does_not_backfill() {
    let h = start(PrivacyPolicy::default());
    // Explicit resume.
    h.set_mode(CaptureMode::Paused {
        until: Timestamp(T0 + 60_000),
    });
    h.fake.copy(text_copy("copied while paused"));
    h.wait_skip(SkipReason::Paused);
    h.set_mode(CaptureMode::Normal);
    // Spurious notifications for the paused change must not capture it.
    h.fake.signal_again(3);
    // Resume by deadline.
    h.set_mode(CaptureMode::Paused {
        until: Timestamp(T0 + 1_000),
    });
    h.fake.copy(text_copy("copied before the deadline"));
    h.wait_skip(SkipReason::Paused);
    h.clock.set(T0 + 2_000);
    h.fake.signal_again(2);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert_eq!(h.fake.snapshot_meta_calls(), 1);
    assert_eq!(h.fake.read_calls(), 1);
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
    assert_eq!(rows.len(), 1);
    let clip = ro.get_clip(&rows[0].id).unwrap().unwrap();
    assert_eq!(clip.preview, "sentinel clip");
}

#[test]
fn paused_passes_no_formats_to_policy() {
    // The gate decides from settings alone (empty markers and formats).
    let now = Timestamp(T0);
    let paused = PrivacyPolicy {
        mode: CaptureMode::Paused {
            until: Timestamp(T0 + 1),
        },
        ..PrivacyPolicy::default()
    };
    assert_eq!(
        gate_without_formats(&paused, now, 5, None, caps()),
        Some(Decision::Skip(SkipReason::Paused))
    );
    let private = PrivacyPolicy {
        mode: CaptureMode::Private,
        ..PrivacyPolicy::default()
    };
    assert_eq!(
        gate_without_formats(&private, now, 5, None, caps()),
        Some(Decision::Skip(SkipReason::Private))
    );
    let normal = PrivacyPolicy::default();
    assert_eq!(gate_without_formats(&normal, now, 5, None, caps()), None);
    assert_eq!(
        gate_without_formats(&normal, now, 5, Some(5), caps()),
        Some(Decision::Skip(SkipReason::SelfWrite))
    );

    // In the actor: a change while private, even with markers, never reaches snapshot_meta.
    let h = start(private);
    let mut c = text_copy("private text");
    c.markers = vec![PrivacyMarker::PasswordManagerHint];
    h.fake.copy(c);
    h.wait_skip(SkipReason::Private);
    h.set_mode(CaptureMode::Paused {
        until: Timestamp(T0 + 60_000),
    });
    h.fake.copy(text_copy("paused text"));
    h.wait_skip(SkipReason::Paused);
    assert_eq!(h.fake.snapshot_meta_calls(), 0);
    assert_eq!(h.fake.read_calls(), 0);
}
