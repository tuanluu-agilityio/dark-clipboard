use super::*;

#[test]
fn self_write_not_recaptured() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("copy me back"));
    let (id, _) = h.wait_added();
    h.copy_back(id, false).unwrap();
    let seen = h.wait_skip(SkipReason::SelfWrite);
    assert!(no_store_events(&seen), "{seen:?}");
    let cur = h.fake.current();
    assert_eq!(cur.len(), 1);
    assert_eq!(cur[0].bytes, b"copy me back");
    assert!(!cur[0].truncated);
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.use_count, 1);
    assert_eq!(clip.copy_count, 1);
    assert_eq!(h.clip_count(), 2);
    assert_eq!(h.fake.read_calls(), 2);
}

#[test]
fn copy_back_plain_writes_only_text() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(FakeCopy {
        formats: vec![
            (
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), 4),
                b"bold".to_vec(),
            ),
            (
                pf("HTML Format", Some(Mime::TextHtml), 11),
                b"<b>bold</b>".to_vec(),
            ),
        ],
        markers: Vec::new(),
        source: None,
    });
    let (id, _) = h.wait_added();
    assert_eq!(
        h.ro().get_clip(&id).unwrap().unwrap().kind,
        ClipType::RichText
    );
    h.copy_back(id, true).unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    let cur = h.fake.current();
    assert_eq!(cur.len(), 1);
    assert_eq!(cur[0].mime, Mime::TextPlain);
    assert_eq!(cur[0].bytes, b"bold");
    h.copy_back(id, false).unwrap();
    h.wait_skip(SkipReason::SelfWrite);
    assert_eq!(h.fake.current().len(), 2);
}

#[test]
fn failed_copy_back_discards_prepared_blocks() {
    // Busy on every attempt (1 + open_retries): the copy-back fails, and the adapter is
    // told once to drop what it prepared for the retries (Task 110 security review).
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("stored earlier"));
    let (id, _) = h.wait_added();
    h.fake.set_busy(test_cfg().open_retries + 1);
    assert_eq!(h.copy_back(id, false), Err(ApiError::ClipboardBusy));
    assert_eq!(h.fake.write_calls(), test_cfg().open_retries + 1);
    assert_eq!(h.fake.discard_calls(), 1);
    // A successful copy-back keeps nothing, so nothing is discarded.
    h.copy_back(id, false).unwrap();
    assert_eq!(h.fake.discard_calls(), 1);
}

#[test]
fn copy_back_unknown_clip_is_not_found() {
    let h = start(PrivacyPolicy::default());
    let id = ClipId::new_v7(Timestamp(T0));
    assert_eq!(h.copy_back(id, false), Err(ApiError::NotFound));
    assert!(h.fake.current().is_empty());
}

#[test]
fn late_copy_back_does_not_write() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("clip A"));
    let (a, _) = h.wait_added();
    h.fake.copy(text_copy("clip B"));
    h.wait_added();
    let before: Vec<_> = h.fake.current().into_iter().map(|r| r.bytes).collect();
    assert_eq!(h.fake.write_calls(), 0);
    let deadline = Instant::now();
    std::thread::sleep(Duration::from_millis(5));
    let (tx, rx) = mpsc::channel();
    h.cap()
        .send(ActorCommand::CopyBack {
            id: a,
            plain: false,
            deadline,
            reply: tx,
        })
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
    assert_eq!(h.fake.write_calls(), 0);
    let now: Vec<_> = h.fake.current().into_iter().map(|r| r.bytes).collect();
    assert_eq!(now, before);
    // On time, the same command writes.
    h.copy_back(a, false).unwrap();
    assert_eq!(h.fake.write_calls(), 1);
}
