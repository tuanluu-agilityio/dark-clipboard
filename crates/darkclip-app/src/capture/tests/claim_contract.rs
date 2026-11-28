use super::claim::{arm, claim_reply, copy_text, long_window, ole_flush, ole_owned, owned};
use super::*;
use crate::capture::claim::ClaimSlot;

// Task 603b: the claim contract fixes from integration review I1.

fn copy_text_if(h: &H, expect_seq: u64, text: &str) -> Result<Option<u64>, ApiError> {
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::CopyTextIf {
            expect_seq,
            text: darkclip_core::Redacted(text.to_string()),
            deadline: Instant::now() + WAIT,
            reply: tx,
        })
        .unwrap();
    rx.recv_timeout(WAIT).unwrap()
}

fn restore_if(h: &H, expect_seq: u64, id: ClipId) -> Result<bool, ApiError> {
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::RestoreIf {
            expect_seq,
            id,
            deadline: Instant::now() + WAIT,
            reply: tx,
        })
        .unwrap();
    rx.recv_timeout(WAIT).unwrap()
}

fn current_text(h: &H) -> String {
    let reps = h.fake.current();
    let r = reps.iter().find(|r| r.mime == Mime::TextPlain).unwrap();
    String::from_utf8(r.bytes.clone()).unwrap()
}

/// Claims `text` copied by `app` with signal time `at`.
fn claim_at(h: &H, app: &str, text: &str, at: i64) -> u64 {
    let rx = arm(h, app);
    let seq = h
        .fake
        .copy_burst_at(vec![(Timestamp(at), owned(app, text))]);
    assert_eq!(claim_reply(&rx), Ok(text.to_string()));
    seq
}

fn translate_skips(h: &H) -> u64 {
    h.cap().skip_counters().get(SkipReason::TranslateSource)
}

#[test]
fn claim_reply_carries_claimed_seq() {
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    let rx = arm(&h, "zalo.exe");
    let seq = h.fake.copy(owned("zalo.exe", "secret words"));
    let r = rx.recv_timeout(WAIT).unwrap().unwrap();
    assert_eq!(r.seq, seq);
    assert_eq!(r.text.0, "secret words");
    let debug = format!("{r:?}");
    assert!(!debug.contains("secret"), "{debug}");
    assert!(debug.contains(&seq.to_string()), "{debug}");
}

#[test]
fn claim_window_is_three_seconds() {
    assert_eq!(CLAIM_WINDOW_MS, 3_000);
    assert_eq!(CLAIM_REPLY_SLACK_MS, 800);
    // The slot's own rule, with exact instants: a change first seen 2900 ms after arming is
    // covered; at the deadline it is not.
    let (tx, _rx) = mpsc::channel();
    let t = Instant::now();
    let slot = ClaimSlot::new(
        "zalo.exe".into(),
        t,
        Duration::from_millis(CLAIM_WINDOW_MS),
        tx,
    );
    assert!(slot.covers(t));
    assert!(slot.covers(t + Duration::from_millis(2_900)));
    assert!(slot.covers(t + Duration::from_millis(2_999)));
    assert!(!slot.covers(t + Duration::from_millis(3_000)));
    // In the actor, with the real window: the actor is held up 1100 ms when it dequeues the
    // target's signal (a round delay past the old 1000 ms window, so it proves the window is
    // longer, with 1900 ms of margin to the new deadline), and still claims it.
    let h = start(PrivacyPolicy::default());
    *h.cap().hooks.stall_on_signal.lock().unwrap() = Some((1, Duration::from_millis(1_100)));
    let rx = arm(&h, "zalo.exe");
    h.fake.copy(owned("zalo.exe", "slow app text"));
    assert_eq!(claim_reply(&rx), Ok("slow app text".to_string()));
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn same_app_follow_up_after_claim_is_skipped_and_counted() {
    // The select-all retry shape: the first Ctrl+C was slow and is claimed; the retry's
    // second copy from the same app follows 450 ms later (signal time).
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    claim_at(&h, "zalo.exe", "first copy", T0);
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 450), owned("zalo.exe", "retry copy"))]);
    let seen = h.wait_skip(SkipReason::TranslateSource);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(translate_skips(&h), 1);
    assert_eq!(h.fake.read_calls(), 1);
    // Up to 3500 ms (burst cap + claim window) after the claimed change's first signal.
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + 3_500),
        owned("zalo.exe", "third copy"),
    )]);
    h.wait_skip(SkipReason::TranslateSource);
    assert_eq!(translate_skips(&h), 2);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 1);
    let previews = h.stored_previews();
    assert!(!previews
        .iter()
        .any(|p| p == "retry copy" || p == "third copy"));
}

#[test]
fn same_app_change_after_window_or_after_own_write_is_stored() {
    // After 3500 ms.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    claim_at(&h, "zalo.exe", "first copy", T0);
    h.fake.copy_burst_at(vec![(
        Timestamp(T0 + 3_501),
        owned("zalo.exe", "later copy"),
    )]);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "later copy");
    assert_eq!(translate_skips(&h), 0);
    // After an own write (the translation), inside the 3500 ms.
    let h = start(PrivacyPolicy::default());
    long_window(&h);
    claim_at(&h, "zalo.exe", "first copy", T0);
    copy_text(&h, "translation").unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    h.fake
        .copy_burst_at(vec![(Timestamp(T0 + 450), owned("zalo.exe", "user copy"))]);
    let (id, _) = h.wait_added();
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().preview, "user copy");
    assert_eq!(translate_skips(&h), 0);
}

#[test]
fn late_flush_skip_is_counted() {
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
    let seen = h.wait_skip(SkipReason::TranslateSource);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(translate_skips(&h), 1);
    assert_eq!(h.fake.read_calls(), 1);
    assert_eq!(h.clip_count(), 0);
}

#[test]
fn copy_text_if_writes_only_when_seq_unchanged() {
    let h = start(PrivacyPolicy::default());
    let claimed = h.fake.copy(text_copy("claimed source"));
    h.wait_added();
    // The user copies something newer: nothing is written over it.
    let newer = h.fake.copy(text_copy("user's newer copy"));
    h.wait_added();
    let writes = h.fake.write_calls();
    assert_eq!(copy_text_if(&h, claimed, "translation"), Ok(None));
    assert_eq!(h.fake.write_calls(), writes);
    assert_eq!(current_text(&h), "user's newer copy");
    // Unchanged: written as our own write.
    assert_eq!(copy_text_if(&h, newer, "translation"), Ok(Some(newer + 1)));
    assert_eq!(h.fake.write_calls(), writes + 1);
    assert_eq!(current_text(&h), "translation");
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert!(!h.stored_previews().iter().any(|p| p == "translation"));
}

#[test]
fn restore_if_writes_only_when_seq_unchanged_and_does_not_mark_used() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("restored clip"));
    let (id, _) = h.wait_added();
    let own = copy_text(&h, "translation").unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    // Moved: the user copied after our write.
    let newer = h.fake.copy(text_copy("user's newer copy"));
    h.wait_added();
    let writes = h.fake.write_calls();
    assert_eq!(restore_if(&h, own, id), Ok(false));
    assert_eq!(h.fake.write_calls(), writes);
    assert_eq!(current_text(&h), "user's newer copy");
    // Unchanged: restored, and not counted as a use.
    assert_eq!(restore_if(&h, newer, id), Ok(true));
    assert_eq!(h.fake.write_calls(), writes + 1);
    assert_eq!(current_text(&h), "restored clip");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.use_count, 0);
    assert_eq!(clip.last_used_at, None);
    // A missing clip.
    let restored = newer + 1;
    let missing = ClipId::new_v7(Timestamp(T0));
    assert_eq!(restore_if(&h, restored, missing), Err(ApiError::NotFound));
    assert_eq!(h.fake.write_calls(), writes + 1);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
}
