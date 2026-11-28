use super::claim::{arm, claim_reply, copy_text, long_window, ole_flush, ole_owned, owned};
use super::*;

// Task 610 (Q-08): a copy from the target app that arrives after the claim window expired
// (Chrome answering the synthetic Ctrl+C late) is skipped for a short tail and never stored.

/// The slot's window in these tests: short, so the expiry is quick (F-16). The tail is
/// measured from the slot's deadline by signal time: `T0 + WINDOW` with the test clock at
/// `T0` when the slot is armed.
const WINDOW: i64 = 50;
/// `UNCLAIMED_TAIL_MS`.
const TAIL: i64 = 5_000;
const _: () = assert!(TAIL as u64 == UNCLAIMED_TAIL_MS);

fn short_window(h: &H) {
    h.cap()
        .hooks
        .claim_window_ms
        .store(u64::try_from(WINDOW).unwrap(), Ordering::SeqCst);
}

fn translate_skips(h: &H) -> u64 {
    h.cap().skip_counters().get(SkipReason::TranslateSource)
}

/// Arms a short slot for `app` and waits for it to expire.
fn expired(h: &H, app: &str) {
    short_window(h);
    let rx = arm(h, app);
    assert_eq!(claim_reply(&rx), Err(ClaimError::Expired));
}

/// A debounce long enough that a change stays pending while the test acts.
fn slow_debounce() -> CaptureConfig {
    CaptureConfig {
        debounce_ms: 1_000,
        ..test_cfg()
    }
}

fn wall_ms() -> i64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(d.as_millis()).unwrap()
}

#[test]
fn late_target_copy_after_expiry_is_skipped_and_counted() {
    let h = start(PrivacyPolicy::default());
    expired(&h, "chrome.exe");
    // Chrome answers the Ctrl+C 500 ms after the deadline (signal time).
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + 500),
        owned("chrome.exe", "late source"),
    )]);
    let seen = h.wait_skip(SkipReason::TranslateSource);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(translate_skips(&h), 1);
    // Decided from metadata only: nothing was read.
    assert_eq!(h.fake.read_calls(), 0);
    // Windows exe names match without case.
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + 900),
        owned("Chrome.EXE", "late again"),
    )]);
    h.wait_skip(SkipReason::TranslateSource);
    assert_eq!(translate_skips(&h), 2);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    let previews = h.stored_previews();
    assert!(!previews
        .iter()
        .any(|p| p == "late source" || p == "late again"));
}

#[test]
fn late_copy_after_tail_or_from_other_app_is_stored() {
    // Another app's change inside the tail is stored.
    let h = start(PrivacyPolicy::default());
    expired(&h, "chrome.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + 500),
        owned("slack.exe", "other app"),
    )]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "other app");
    assert_eq!(translate_skips(&h), 0);
    // The target's change 5001 ms after the deadline is stored.
    let h = start(PrivacyPolicy::default());
    expired(&h, "chrome.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + TAIL + 1),
        owned("chrome.exe", "after tail"),
    )]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "after tail");
    assert_eq!(translate_skips(&h), 0);
    // At 5000 ms exactly it is still in the tail.
    let h = start(PrivacyPolicy::default());
    expired(&h, "chrome.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + TAIL),
        owned("chrome.exe", "tail edge"),
    )]);
    h.wait_skip(SkipReason::TranslateSource);
    assert_eq!(translate_skips(&h), 1);
}

#[test]
fn own_write_ends_the_tail() {
    let h = start(PrivacyPolicy::default());
    expired(&h, "chrome.exe");
    copy_text(&h, "translation").unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + 500),
        owned("chrome.exe", "user copy"),
    )]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "user copy");
    assert_eq!(translate_skips(&h), 0);
}

#[test]
fn new_arm_for_same_app_keeps_tail_and_new_claim_wins() {
    // The late copy of the first press is pending (long debounce) when the second press arms
    // for the same app: the new slot cannot claim it (seen before arming), the kept tail
    // skips it, and the second press's own copy is claimed.
    for second_app in ["chrome.exe", "slack.exe"] {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            slow_debounce(),
            |_| {},
        );
        // Signals here carry wall time: the test clock follows it so the tail applies.
        h.clock.set(wall_ms());
        expired(&h, "chrome.exe");
        h.fake
            .copy_then_set_source(text_copy("late source"), Some(src("chrome.exe")));
        let deadline = Instant::now() + WAIT;
        while h.fake.resolve_calls() == 0 {
            assert!(Instant::now() < deadline, "signal never dequeued");
            std::thread::sleep(Duration::from_millis(1));
        }
        long_window(&h);
        let rx = arm(&h, second_app);
        // Kept, whatever app the second press is for (tails are per app): the late copy
        // is skipped, never stored.
        let seen = h.wait_skip(SkipReason::TranslateSource);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(translate_skips(&h), 1);
        assert_eq!(h.fake.read_calls(), 0);
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        if second_app == "chrome.exe" {
            // The new slot wins for the next change from the target.
            h.fake.copy(owned("chrome.exe", "second press"));
            assert_eq!(claim_reply(&rx), Ok("second press".to_string()));
            assert_eq!(translate_skips(&h), 1);
            let between = h.sentinel();
            assert!(no_store_events(&between), "{between:?}");
            assert_eq!(h.clip_count(), 1);
        }
        assert!(!h.stored_previews().iter().any(|p| p == "late source"));
    }
}

#[test]
fn arm_for_other_app_replaces_live_slot_and_both_late_copies_are_skipped() {
    // Press in A, Esc during the copy wait (A's slot stays live), then press in B: B's arm
    // replaces A's slot (A's requester gets `Expired`) instead of replying `Busy`.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx_a = arm(&h, "zalo.exe");
    short_window(&h);
    let rx_b = arm(&h, "chrome.exe");
    assert_eq!(claim_reply(&rx_a), Err(ClaimError::Expired));
    // A's late copy: skipped by A's tail.
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 500), owned("zalo.exe", "late a"))]);
    h.wait_skip(SkipReason::TranslateSource);
    // B's slot expires unclaimed; B's late copy is skipped by B's tail, and A's tail is
    // still there.
    assert_eq!(claim_reply(&rx_b), Err(ClaimError::Expired));
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 600), owned("chrome.exe", "late b"))]);
    h.wait_skip(SkipReason::TranslateSource);
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + 700),
        owned("zalo.exe", "late a again"),
    )]);
    h.wait_skip(SkipReason::TranslateSource);
    assert_eq!(translate_skips(&h), 3);
    assert_eq!(h.fake.read_calls(), 0);
    // A new press in B claims B's copy.
    long_window(&h);
    let rx = arm(&h, "chrome.exe");
    h.fake.copy(owned("chrome.exe", "claimed b"));
    assert_eq!(claim_reply(&rx), Ok("claimed b".to_string()));
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    let previews = h.stored_previews();
    for text in ["late a", "late b", "late a again", "claimed b"] {
        assert!(!previews.iter().any(|p| p == text), "{text}");
    }
}

#[test]
fn tails_are_capped_at_four() {
    let h = start(PrivacyPolicy::default());
    let apps = ["a1.exe", "a2.exe", "a3.exe", "a4.exe", "a5.exe"];
    for app in apps {
        expired(&h, app);
    }
    // The oldest tail (a1) was dropped: its late copy is stored.
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 500), owned("a1.exe", "late a1"))]);
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "late a1");
    assert_eq!(translate_skips(&h), 0);
    // The four newest are kept.
    for (i, app) in apps[1..].iter().enumerate() {
        let at = T0 + 600 + i64::try_from(i).unwrap() * 10;
        h.fake
            .copy_burst_at(vec![(Timestamp(at), owned(app, "late copy"))]);
        h.wait_skip(SkipReason::TranslateSource);
    }
    assert_eq!(translate_skips(&h), 4);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn change_first_seen_inside_tail_is_skipped_when_processed_after_it() {
    // A 1 s tail. The change is first seen right after the expiry (about 1 s of margin),
    // then the actor is held up 2 s by a repeat signal of the same change while it is
    // pending, so it is processed well after the tail's actor-time end.
    const TAIL_MS: u64 = 1_000;
    const STALL: Duration = Duration::from_millis(2_000);
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        slow_debounce(),
        |_| {},
    );
    h.cap()
        .hooks
        .unclaimed_tail_ms
        .store(TAIL_MS, Ordering::SeqCst);
    // Signals here carry wall time: the test clock follows it so the tail applies.
    let t0 = wall_ms();
    h.clock.set(t0);
    let started = Instant::now();
    expired(&h, "chrome.exe");
    // The signal has no owner: the actor resolves it when it first sees the change, so
    // `resolve_calls` tells the test the change is pending.
    let seq = h
        .fake
        .copy_then_set_source(text_copy("late source"), Some(src("chrome.exe")));
    let deadline = Instant::now() + WAIT;
    while h.fake.resolve_calls() == 0 {
        assert!(Instant::now() < deadline, "signal never dequeued");
        std::thread::sleep(Duration::from_millis(1));
    }
    let first_seen = started.elapsed();
    *h.cap().hooks.stall_on_signal.lock().unwrap() = Some((seq, STALL));
    h.fake.signal_again(1);
    let seen = h.wait_skip(SkipReason::TranslateSource);
    assert!(no_store_events(&seen), "{seen:?}");
    // Seen inside the tail, processed after it ended (by actor time).
    assert!(
        first_seen < Duration::from_millis(TAIL_MS),
        "{first_seen:?}"
    );
    let processed = started.elapsed();
    assert!(
        processed > Duration::from_millis(WINDOW as u64 + TAIL_MS),
        "{processed:?}"
    );
    // A change first seen after the tail (its signal time still inside the signal-time
    // bound): stored.
    h.fake.copy_burst_at(vec![(
        Timestamp(t0 + WINDOW + 60),
        owned("chrome.exe", "after tail"),
    )]);
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "after tail");
}

#[test]
fn late_ole_flush_naming_target_is_skipped() {
    // Notepad shape in one burst: the owned phase, then its ownerless flush, whose F-14 heir
    // names the target.
    let h = start_with(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        slow_debounce(),
        |_| {},
    );
    expired(&h, "notepad.exe");
    h.fake.copy_burst_at(vec![
        (
            Timestamp(T0 + WINDOW + 500),
            ole_owned("notepad.exe", "late text"),
        ),
        (Timestamp(T0 + WINDOW + 510), ole_flush("late text")),
    ]);
    let seen = h.wait_skip(SkipReason::TranslateSource);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "late text"));

    // Across two bursts: the owned phase is skipped by the tail, then its flush arrives
    // 20 ms later (signal time) with no heir. It is skipped too.
    let h = start(PrivacyPolicy::default());
    expired(&h, "notepad.exe");
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + WINDOW + 500),
        ole_owned("notepad.exe", "late text"),
    )]);
    h.wait_skip(SkipReason::TranslateSource);
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + WINDOW + 520), ole_flush("late text"))]);
    h.wait_skip(SkipReason::TranslateSource);
    assert_eq!(translate_skips(&h), 2);
    assert_eq!(h.fake.read_calls(), 0);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    assert!(!h.stored_previews().iter().any(|p| p == "late text"));
}
