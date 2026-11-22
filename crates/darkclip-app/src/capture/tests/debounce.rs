use super::*;

#[test]
fn burst_same_seq_stores_once() {
    let h = start(PrivacyPolicy::default());
    h.fake.copy(text_copy("burst text"));
    h.fake.signal_again(19);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 2);
    assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().copy_count, 1);
    assert_eq!(h.fake.read_calls(), 2);
}

#[test]
fn ole_three_phase_keeps_first_source() {
    let h = start(PrivacyPolicy::default());
    // Phase 1: delayed rendering, placeholders only, owner known.
    h.fake.copy(FakeCopy {
        formats: vec![
            (pf("DataObject", None, 8), Vec::new()),
            (pf("Ole Private Data", None, 8), Vec::new()),
        ],
        markers: Vec::new(),
        source: Some(src("winword.exe")),
    });
    // Sync point: the actor has seen phase 1 (nothing readable yet). Without it the
    // test thread could clear the owner before the actor dequeues the first signal.
    let seen = h.wait_skip(SkipReason::NothingReadable);
    assert!(no_store_events(&seen), "{seen:?}");
    assert_eq!(h.fake.read_calls(), 0);
    // Phase 2: real formats under the same seq.
    h.fake.render(vec![
        (pf("DataObject", None, 8), Vec::new()),
        (
            pf("CF_UNICODETEXT", Some(Mime::TextPlain), 8),
            b"ole text".to_vec(),
        ),
    ]);
    // Phase 3: flush, owner gone.
    h.fake.clear_source();
    h.fake.signal_again(1);
    let (id, before) = h.wait_added();
    assert!(no_store_events(&before), "{before:?}");
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(
        clip.source.as_ref().map(|s| s.app_id.as_str()),
        Some("winword.exe")
    );
    let between = h.sentinel();
    assert!(no_store_events(&between), "{between:?}");
    assert_eq!(h.clip_count(), 2);
}

#[test]
fn signal_storm_is_capped() {
    let cfg = CaptureConfig {
        debounce_ms: 10,
        ..test_cfg()
    };
    let h = start_full(
        PrivacyPolicy::default(),
        RetentionPolicy::default(),
        cfg,
        Duration::from_millis(40),
        |_| {},
    );
    h.fake.copy(text_copy("noisy writer"));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let storm = {
        let (fake, stop, ended) = (h.fake.clone(), Arc::clone(&stop), Arc::clone(&ended));
        std::thread::spawn(move || {
            // Same seq every 0.5 ms (busy-wait: OS sleeps may be coarser than the debounce).
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(300) {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                fake.signal_again(1);
                let t = Instant::now();
                while t.elapsed() < Duration::from_micros(500) {
                    std::hint::spin_loop();
                }
            }
            ended.store(true, Ordering::SeqCst);
        })
    };
    h.wait_added();
    let storm_was_running = !ended.load(Ordering::SeqCst);
    stop.store(true, Ordering::SeqCst);
    storm.join().unwrap();
    assert!(storm_was_running, "capture waited for the storm to end");
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn ole_three_phases_in_one_window() {
    in_one_window(|| {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            slow_cfg(),
            |_| {},
        );
        h.fake.copy(FakeCopy {
            formats: vec![
                (pf("DataObject", None, 8), Vec::new()),
                (pf("Ole Private Data", None, 8), Vec::new()),
            ],
            markers: Vec::new(),
            source: Some(src("winword.exe")),
        });
        // The first signal carries winword.exe (F-01): phases 2 and 3 may land before
        // or after the actor dequeues it.
        h.fake.render(vec![
            (pf("DataObject", None, 8), Vec::new()),
            (
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), 8),
                b"ole text".to_vec(),
            ),
        ]);
        h.fake.clear_source();
        h.fake.signal_again(1);
        let (id, before) = h.wait_added();
        if h.fake.snapshot_meta_calls() > 1 {
            return false; // Window closed: phase 1 was inspected alone.
        }
        // One window: phase 1 was never inspected on its own.
        assert!(before.is_empty(), "{before:?}");
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.preview, "ole text");
        assert_eq!(
            clip.source.as_ref().map(|s| s.app_id.as_str()),
            Some("winword.exe")
        );
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 2);
        true
    });
}
