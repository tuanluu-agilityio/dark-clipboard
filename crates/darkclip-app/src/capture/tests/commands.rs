use super::*;

#[test]
fn shutdown_within_deadline() {
    let mut h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("before shutdown"));
    h.wait_added();
    let cap = h.cap.take().unwrap();
    // Queued before Shutdown: must be drained, not lost.
    cap.send(ActorCommand::SetMode(CaptureMode::Private))
        .unwrap();
    let t = Instant::now();
    cap.shutdown(Duration::from_secs(2)).unwrap();
    assert!(t.elapsed() < Duration::from_secs(2));
    // `shutdown` consumes the handle, so no later `send` can compile; the actor is gone:
    // the drained event arrived and then the event channel closed.
    assert_eq!(
        h.events.recv_timeout(WAIT),
        Ok(AppEvent::ModeChanged(CaptureMode::Private))
    );
    assert_eq!(
        h.events.recv_timeout(WAIT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
    // The monitor was stopped: a later copy reaches nobody.
    let (meta, reads) = (h.fake.snapshot_meta_calls(), h.fake.read_calls());
    h.fake.copy(text_copy("after shutdown"));
    h.fake.signal_again(3);
    assert_eq!(h.fake.snapshot_meta_calls(), meta);
    assert_eq!(h.fake.read_calls(), reads);
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn masked_preview_uses_settings_language() {
    let policy = PrivacyPolicy {
        sensitive_handling: SensitiveHandling::StoreMasked,
        ..PrivacyPolicy::default()
    };
    let enabled = policy.sensitive_categories_enabled.clone();
    let cfg = CaptureConfig {
        lang: Lang::Vi,
        ..test_cfg()
    };
    let h = start_with(policy, RetentionPolicy::default(), cfg, |_| {});
    let text = api_key_text();
    h.fake.copy(text_copy(&text));
    let (id, _) = h.wait_added();
    let hint = scan(&text, &enabled).mask_hint.unwrap();
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.preview, masked_preview(&hint, Lang::Vi));
    assert_ne!(clip.preview, masked_preview(&hint, Lang::En));
    // A language change applies to later captures.
    h.cap().send(ActorCommand::SetLang(Lang::En)).unwrap();
    h.fake
        .copy(text_copy(&format!("other {}", &api_key_text()[4..])));
    let (id2, _) = h.wait_added();
    let clip2 = h.ro().get_clip(&id2).unwrap().unwrap();
    let hint2 = scan(&format!("other {}", &api_key_text()[4..]), &enabled)
        .mask_hint
        .unwrap();
    assert_eq!(clip2.preview, masked_preview(&hint2, Lang::En));
}

#[test]
fn late_commands_have_no_side_effect() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("keep me"));
    let (id, _) = h.wait_added();
    let deadline = Instant::now();
    std::thread::sleep(Duration::from_millis(5));
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::DeleteClip {
            id,
            deadline,
            reply: tx,
        })
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::SetPinned {
            id,
            pinned: true,
            deadline,
            reply: tx,
        })
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::ClearHistory {
            keep_pinned: false,
            deadline,
            reply: tx,
        })
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
    assert_eq!(h.clip_count(), 1);
    assert!(!h.ro().get_clip(&id).unwrap().unwrap().pinned);
}
