//! Task 605: the translate-in-place wrappers over the capture claim commands.

use super::*;
use darkclip_core::Redacted;

const TARGET: &str = "target-editor.exe";

fn owned(text: &str) -> FakeCopy {
    FakeCopy {
        source: Some(ClipboardSource {
            app_id: TARGET.into(),
            app_name: None,
            platform: Platform::Windows,
            confidence: SourceConfidence::Owner,
        }),
        ..text_copy(text)
    }
}

fn others() -> Vec<Caller> {
    vec![
        Caller::Cli,
        Caller::Mcp {
            client_id: "c".into(),
        },
    ]
}

#[test]
fn translate_api_is_ui_only() {
    let h = start(&[("previous", false)], None);
    let a = h.app();
    for c in others() {
        let denied = |r: Result<(), ApiError>| matches!(r, Err(ApiError::PermissionDenied(_)));
        assert!(denied(a.ui_translate_claim(&c, TARGET.into()).map(|_| ())));
        assert!(denied(a.ui_restore_candidate(&c).map(|_| ())));
        assert!(denied(
            a.ui_copy_text_if(&c, 1, Redacted("x".into())).map(|_| ())
        ));
        assert!(denied(a.ui_restore_if(&c, 1, h.ids[0]).map(|_| ())));
        assert!(denied(a.ui_clipboard_seq(&c).map(|_| ())));
    }
    // Nothing was written for them.
    assert_eq!(h.fake.write_calls(), 0);
}

#[test]
fn clipboard_seq_follows_the_clipboard() {
    let h = start(&[], None);
    let a = h.app();
    let before = a.ui_clipboard_seq(&Caller::Ui).unwrap();
    let seq = h.fake.copy(text_copy("hello"));
    assert_ne!(seq, before);
    assert_eq!(a.ui_clipboard_seq(&Caller::Ui).unwrap(), seq);
}

#[test]
fn claim_write_and_restore_through_the_api() {
    let h = start(&[], None);
    let a = h.app();
    // The previous clipboard is a stored clip: the restore candidate.
    h.fake.copy(text_copy("previous clipboard"));
    let id = h.wait_added();
    let candidate = a.ui_restore_candidate(&Caller::Ui).unwrap();
    assert_eq!(candidate, Some(id));

    // Arming returns at once; the target's copy is claimed, not stored.
    let rx = a.ui_translate_claim(&Caller::Ui, TARGET.into()).unwrap();
    let seq = h.fake.copy(owned("xin chào"));
    let claimed = rx.recv_timeout(WAIT).unwrap().unwrap();
    assert_eq!(claimed.seq, seq);
    assert_eq!(claimed.text.0, "xin chào");
    assert_eq!(a.ui_clipboard_seq(&Caller::Ui).unwrap(), seq);

    // A stale seq writes nothing.
    assert_eq!(
        a.ui_copy_text_if(&Caller::Ui, seq + 100, Redacted("hello".into()))
            .unwrap(),
        None
    );
    let own = a
        .ui_copy_text_if(&Caller::Ui, seq, Redacted("hello".into()))
        .unwrap()
        .unwrap();
    assert_eq!(h.fake.current()[0].bytes, b"hello");
    // Restore only over our own write.
    assert!(!a.ui_restore_if(&Caller::Ui, own + 100, id).unwrap());
    assert!(a.ui_restore_if(&Caller::Ui, own, id).unwrap());
    assert_eq!(h.fake.current()[0].bytes, b"previous clipboard");
    // Only the previous clipboard is in the history.
    assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 1);
}

#[test]
fn a_second_claim_replaces_the_armed_one() {
    // Task 610: never two live claims; the older one ends `Expired` at once.
    let h = start(&[], None);
    let a = h.app();
    let first = a.ui_translate_claim(&Caller::Ui, TARGET.into()).unwrap();
    let second = a.ui_translate_claim(&Caller::Ui, TARGET.into()).unwrap();
    assert_eq!(
        first.recv_timeout(WAIT).unwrap().unwrap_err(),
        crate::capture::ClaimError::Expired
    );
    assert!(second.try_recv().is_err());
}

#[test]
fn is_excluded_app_uses_the_current_list_with_capture_matching() {
    let h = start(&[], None);
    let a = h.app();
    let platform = crate::api::translate::translate_platform();
    assert!(!a.is_excluded_app("private-notes.exe"));
    let mut s = a.settings();
    s.privacy.excluded_apps.push(darkclip_core::ExcludedApp {
        app_id: "Private-Notes.exe".into(),
        platform,
        name: None,
        builtin: false,
    });
    a.update_settings(s.clone()).unwrap();
    let expected = darkclip_core::policy::app_is_excluded(
        "private-notes.exe",
        platform,
        &s.privacy.excluded_apps,
    );
    assert_eq!(a.is_excluded_app("private-notes.exe"), expected);
    if platform == Platform::Windows {
        assert!(a.is_excluded_app("private-notes.exe"));
        // The built-in exclusions count too.
        let builtin = s
            .privacy
            .excluded_apps
            .iter()
            .find(|e| e.builtin && e.platform == Platform::Windows)
            .unwrap()
            .app_id
            .clone();
        assert!(a.is_excluded_app(&builtin));
    }
    assert!(!a.is_excluded_app(""));
}
