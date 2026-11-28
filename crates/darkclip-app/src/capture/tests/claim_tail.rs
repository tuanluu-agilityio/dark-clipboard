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
        if second_app == "chrome.exe" {
            // Kept: the late copy is skipped, never stored.
            let seen = h.wait_skip(SkipReason::TranslateSource);
            assert!(no_store_events(&seen), "{seen:?}");
            assert_eq!(translate_skips(&h), 1);
            assert_eq!(h.fake.read_calls(), 0);
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
            // The new slot wins for the next change from the target.
            h.fake.copy(owned("chrome.exe", "second press"));
            assert_eq!(claim_reply(&rx), Ok("second press".to_string()));
            assert_eq!(translate_skips(&h), 1);
            let between = h.sentinel();
            assert!(no_store_events(&between), "{between:?}");
            assert_eq!(h.clip_count(), 1);
        } else {
            // Arming for another app clears the tail: the change is stored as before.
            let (id, _) = h.wait_added();
            assert_eq!(
                h.ro().get_clip(&id).unwrap().unwrap().preview,
                "late source"
            );
            assert_eq!(translate_skips(&h), 0);
        }
    }
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
