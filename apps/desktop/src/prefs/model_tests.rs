//! Tests of the settings view-model (Task 207).

use super::model::*;
use darkclip_app::settings::Settings;
use darkclip_core::i18n::Msg;
use darkclip_core::policy::SkipReason;
use darkclip_core::{ClipType, Lang, Platform, SensitiveHandling};
use std::collections::BTreeMap;

fn defaults() -> Settings {
    Settings::default()
}

fn msgs(r: Result<Settings, Vec<Problem>>) -> Vec<(Field, Msg)> {
    r.unwrap_err()
        .into_iter()
        .map(|p| (p.field, p.msg))
        .collect()
}

#[test]
fn an_untouched_draft_saves_the_same_settings() {
    let base = defaults();
    let d = Draft::from_settings(&base);
    assert_eq!(d, Draft::from_settings(&base));
    assert_eq!(d.apply_to(&base).unwrap(), base);
}

#[test]
fn numbers_are_checked_with_their_own_messages() {
    let base = defaults();
    let mut d = Draft::from_settings(&base);
    d.max_items = "abc".into();
    d.max_age_days = "3651".into();
    d.max_storage_mb = "9".into();
    assert_eq!(
        msgs(d.apply_to(&base)),
        vec![
            (Field::MaxItems, Msg::SettingsErrNumber),
            (Field::MaxAgeDays, Msg::SettingsErrRange),
            (Field::MaxStorageMb, Msg::SettingsErrRange),
        ]
    );
    for bad in [
        "",
        " ",
        "-5",
        "+5",
        "1.5",
        "99999999999999999999",
        "1000001",
    ] {
        d.max_items = bad.into();
        d.max_age_days = "30".into();
        d.max_storage_mb = "500".into();
        let m = msgs(d.apply_to(&base));
        assert_eq!(m.len(), 1, "{bad:?}");
        assert_eq!(m[0].0, Field::MaxItems);
    }
    d.max_items = " 250 ".into();
    assert_eq!(d.apply_to(&base).unwrap().retention.max_items, 250);
}

#[test]
fn range_edges_are_accepted() {
    let base = defaults();
    let mut d = Draft::from_settings(&base);
    d.max_items = "0".into();
    d.max_age_days = "0".into();
    d.max_storage_mb = "102400".into();
    let s = d.apply_to(&base).unwrap();
    assert_eq!(
        (
            s.retention.max_items,
            s.retention.max_age_days,
            s.retention.max_storage_mb
        ),
        (0, 0, 102_400)
    );
    d.max_items = "1000000".into();
    d.max_age_days = "3650".into();
    d.max_storage_mb = "10".into();
    assert!(d.apply_to(&base).is_ok());
    d.max_storage_mb = "9".into();
    assert!(d.apply_to(&base).is_err());
    d.max_storage_mb = "10".into();
    d.max_items = "1000001".into();
    assert!(d.apply_to(&base).is_err());
}

#[test]
fn every_value_the_schema_allows_saves_back_unchanged() {
    for (i, a, m) in [(0, 0, 10), (1_000_000, 3650, 102_400), (1, 1, 11)] {
        let mut base = defaults();
        base.retention.max_items = i;
        base.retention.max_age_days = a;
        base.retention.max_storage_mb = m;
        assert_eq!(Draft::from_settings(&base).apply_to(&base).unwrap(), base);
    }
}

#[test]
fn a_bad_shortcut_is_refused_with_the_shortcut_message() {
    let base = defaults();
    let mut d = Draft::from_settings(&base);
    d.shortcut = "V".into();
    assert_eq!(
        msgs(d.apply_to(&base)),
        vec![(Field::Shortcut, Msg::ShortcutErrNoModifier)]
    );
    d.shortcut = "ctrl + alt + k".into();
    let s = d.apply_to(&base).unwrap();
    assert_eq!(s.shortcuts.toggle_popup, "Ctrl+Alt+K");
}

#[test]
fn at_least_one_kind_must_stay_on() {
    let base = defaults();
    let mut d = Draft::from_settings(&base);
    d.kinds = [false; 7];
    assert_eq!(
        msgs(d.apply_to(&base)),
        vec![(Field::Kinds, Msg::SettingsErrNoKinds)]
    );
    d.kinds[5] = true;
    let s = d.apply_to(&base).unwrap();
    assert_eq!(s.privacy.capture_kinds, vec![ClipType::Image]);
}

#[test]
fn a_save_keeps_what_the_form_does_not_show() {
    let mut base = defaults();
    base.ui.history_windows.insert(
        "k".into(),
        darkclip_app::settings::WindowRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        },
    );
    base.privacy.mode = darkclip_core::CaptureMode::Private;
    let mut d = Draft::from_settings(&base);
    d.enter_pastes = true;
    d.language = Lang::Vi;
    d.sensitive = SensitiveHandling::StoreMasked;
    let s = d.apply_to(&base).unwrap();
    assert_eq!(s.ui.history_windows, base.ui.history_windows);
    assert_eq!(s.privacy.mode, base.privacy.mode);
    assert_eq!(
        s.privacy.sensitive_categories_enabled,
        base.privacy.sensitive_categories_enabled
    );
    assert!(s.ui.enter_pastes);
    assert_eq!(s.ui.language, Lang::Vi);
    assert_eq!(s.privacy.sensitive_handling, SensitiveHandling::StoreMasked);
}

#[test]
fn built_in_apps_go_only_after_an_explicit_confirmation() {
    let base = defaults();
    let mut list = AppList::new(base.privacy.excluded_apps.clone());
    let n = list.items().len();
    let builtin = list.items().iter().position(|a| a.builtin).unwrap();
    let id = list.items()[builtin].app_id.clone();
    // Asking changes nothing.
    assert_eq!(
        list.request_remove(builtin),
        RemoveOutcome::NeedsConfirmation
    );
    assert_eq!(list.items().len(), n);
    assert_eq!(list.pending().map(|a| a.app_id.clone()), Some(id.clone()));
    // Cancelling keeps it.
    list.cancel_remove();
    assert_eq!(list.items().len(), n);
    assert!(list.pending().is_none());
    // Confirming without asking first does nothing.
    assert!(!list.confirm_remove());
    assert_eq!(list.items().len(), n);
    // Ask, then confirm: gone.
    list.request_remove(builtin);
    assert!(list.confirm_remove());
    assert_eq!(list.items().len(), n - 1);
    assert!(list.items().iter().all(|a| a.app_id != id));
}

#[test]
fn a_custom_app_goes_at_once_and_a_new_request_drops_a_pending_one() {
    let mut list = AppList::new(defaults().privacy.excluded_apps);
    list.add("tool.exe", Platform::Windows).unwrap();
    let builtin = list.items().iter().position(|a| a.builtin).unwrap();
    list.request_remove(builtin);
    let custom = list.items().len() - 1;
    assert_eq!(list.request_remove(custom), RemoveOutcome::Removed);
    assert!(list.pending().is_none());
    assert!(!list.confirm_remove());
    assert_eq!(list.request_remove(999), RemoveOutcome::NotFound);
}

#[test]
fn added_apps_are_program_names_only() {
    let mut list = AppList::default();
    assert_eq!(
        list.add("  Vault.EXE ", Platform::Windows),
        Ok("Vault.EXE".to_string())
    );
    assert_eq!(
        list.add("vault.exe", Platform::Windows),
        Err(Msg::SettingsErrAppDuplicate)
    );
    assert_eq!(
        list.add("   ", Platform::Windows),
        Err(Msg::SettingsErrAppEmpty)
    );
    for bad in [
        "C:\\Tools\\a.exe",
        "dir/a",
        "a\u{0}b",
        "a\nb",
        &"x".repeat(256),
    ] {
        assert_eq!(
            list.add(bad, Platform::Windows),
            Err(Msg::SettingsErrAppInvalid),
            "{bad:?}"
        );
    }
    assert_eq!(list.items().len(), 1);
    let added = &list.items()[0];
    assert!(!added.builtin);
    assert_eq!(added.name, None);
}

#[test]
fn the_markers_row_is_locked_text_and_a_save_cannot_switch_them_off() {
    let rows = privacy_rows();
    let locked: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r, PrivacyRow::Locked { .. }))
        .collect();
    assert_eq!(locked.len(), 1);
    assert_eq!(
        *locked[0],
        PrivacyRow::Locked {
            label: Msg::SettingsMarkersLocked,
            note: Msg::SettingsMarkersNote
        }
    );
    // No editable control is about the markers: the controls are exactly these four.
    let editable: Vec<Control> = rows
        .iter()
        .filter_map(|r| match r {
            PrivacyRow::Editable(c) => Some(*c),
            PrivacyRow::Locked { .. } => None,
        })
        .collect();
    assert_eq!(
        editable,
        vec![
            Control::CaptureMode,
            Control::Sensitive,
            Control::Kinds,
            Control::ExcludedApps
        ]
    );
    // Even a base that says "off" (it cannot be loaded that way) comes out on.
    let mut base = defaults();
    base.privacy.respect_os_markers = false;
    let out = Draft::from_settings(&base).apply_to(&base).unwrap();
    assert!(out.privacy.respect_os_markers);
}

#[test]
fn read_only_shows_the_banner_and_blocks_saving() {
    assert_eq!(banner(true), Some(Msg::SettingsReadOnlyBanner));
    assert_eq!(banner(false), None);
    assert!(!can_save(true, false));
    assert!(!can_save(false, true));
    assert!(can_save(false, false));
}

#[test]
fn clearing_the_history_needs_the_dialog_and_keeps_pinned_by_default() {
    let mut f = ClearFlow::default();
    assert_eq!(f.confirm(), None);
    f.ask();
    assert!(f.is_open());
    f.cancel();
    assert_eq!(f.confirm(), None);
    f.ask();
    assert_eq!(f.confirm(), Some(true));
    // Closed after a confirmation: a second confirm does nothing.
    assert_eq!(f.confirm(), None);
    f.ask();
    f.set_keep_pinned(false);
    assert_eq!(f.confirm(), Some(false));
    // Changing the option with no dialog open does not open one.
    f.set_keep_pinned(false);
    assert!(!f.is_open());
}

#[test]
fn every_skip_reason_has_a_message_and_zero_counts_are_hidden() {
    use SkipReason::*;
    for r in [
        Disabled,
        Paused,
        Private,
        SelfWrite,
        OsConcealed,
        DarkshotExcluded,
        Transient,
        ExcludedApp,
        KindDisabled,
        TooLarge,
        NothingReadable,
        Sensitive,
        Busy,
        TranslateSource,
    ] {
        assert!(skip_msg(r.name()).is_some(), "{}", r.name());
    }
    let mut m = BTreeMap::new();
    m.insert("os_concealed".to_string(), 3);
    m.insert("busy".to_string(), 0);
    m.insert("unknown_reason".to_string(), 9);
    assert_eq!(skip_rows(&m), vec![(Msg::SkipOsConcealed, 3)]);
}

#[test]
fn duplicates_use_the_same_unicode_lowercase_as_policy_matching() {
    let mut list = AppList::default();
    list.add("\u{C9}tude.exe", Platform::Windows).unwrap();
    assert_eq!(
        list.add("\u{E9}tude.EXE", Platform::Windows),
        Err(Msg::SettingsErrAppDuplicate)
    );
}

#[test]
fn excluded_apps_are_shown_without_direction_controls() {
    // A hand-edited settings file can hold anything (F-18).
    let app = darkclip_core::ExcludedApp {
        app_id: "invoice\u{202E}fdp.exe".into(),
        platform: Platform::Windows,
        name: Some("Pay\u{2066}roll\u{200F}".into()),
        builtin: false,
    };
    let (name, id) = app_display(&app);
    assert_eq!(id, "invoice\u{FFFD}fdp.exe");
    assert_eq!(name, "Pay\u{FFFD}roll\u{FFFD}");
    let plain = darkclip_core::ExcludedApp {
        name: None,
        app_id: "keepassxc.exe".into(),
        ..app
    };
    assert_eq!(app_display(&plain), (String::new(), "keepassxc.exe".into()));
}

#[test]
fn the_darkshot_entry_has_its_own_removal_question() {
    let builtins = defaults().privacy.excluded_apps;
    let find = |id: &str| builtins.iter().find(|a| a.app_id == id).unwrap().clone();
    assert_eq!(
        remove_builtin_msg(&find("darkshot.exe")),
        Msg::SettingsConfirmRemoveDarkshot
    );
    for id in [
        "keepassxc.exe",
        "keepass.exe",
        "1password.exe",
        "bitwarden.exe",
    ] {
        assert_eq!(
            remove_builtin_msg(&find(id)),
            Msg::SettingsConfirmRemoveBuiltin
        );
    }
    for lang in [Lang::En, Lang::Vi] {
        let t = |m| darkclip_core::i18n::t(lang, m);
        assert_ne!(
            t(Msg::SettingsConfirmRemoveDarkshot),
            t(Msg::SettingsConfirmRemoveBuiltin)
        );
        assert!(t(Msg::SettingsConfirmRemoveDarkshot).contains("Darkshot"));
    }
    // The Darkshot text never says its copies will be stored again.
    let en = darkclip_core::i18n::t(Lang::En, Msg::SettingsConfirmRemoveDarkshot);
    assert!(!en.contains("stored again"));
    assert!(en.contains("excluded"));
}

#[test]
fn direction_controls_are_refused_in_program_names() {
    let mut list = AppList::default();
    assert_eq!(
        list.add("invoice\u{202E}fdp.exe", Platform::Windows),
        Err(Msg::SettingsErrAppInvalid)
    );
}

#[test]
fn a_read_only_settings_file_is_never_saved_and_never_reported_as_saved() {
    let base = defaults();
    let d = Draft::from_settings(&base);
    let mut called = false;
    let r = run_save_with_login(true, &d, &base, None, |_| {
        called = true;
        true
    });
    assert!(matches!(r, SaveResult::ReadOnly));
    assert!(!called);
    assert!(matches!(
        run_save_with_login(false, &d, &base, None, |_| true),
        SaveResult::Saved
    ));
    assert!(matches!(
        run_save_with_login(false, &d, &base, None, |_| false),
        SaveResult::Failed
    ));
    let mut bad = d.clone();
    bad.max_items = "x".into();
    assert!(matches!(
        run_save_with_login(false, &bad, &base, None, |_| true),
        SaveResult::Invalid(_)
    ));
}

#[test]
fn lowering_a_retention_limit_is_detected() {
    let base = defaults().retention;
    let with = |i: u32, a: u32, m: u32| {
        let mut r = base.clone();
        r.max_items = i;
        r.max_age_days = a;
        r.max_storage_mb = m;
        r
    };
    let old = with(1000, 30, 500);
    assert!(!limits_lowered(&old, &old));
    assert!(!limits_lowered(&old, &with(2000, 60, 900)));
    assert!(limits_lowered(&old, &with(999, 30, 500)));
    assert!(limits_lowered(&old, &with(1000, 29, 500)));
    assert!(limits_lowered(&old, &with(1000, 30, 499)));
    // 0 = unlimited: going to a number lowers, going to 0 raises.
    assert!(limits_lowered(&with(0, 0, 500), &with(10, 30, 500)));
    assert!(!limits_lowered(&old, &with(0, 0, 500)));
}

mod login {
    use super::*;
    use darkclip_platform::fake::FakeAutostart;
    use darkclip_platform::Autostart;
    use std::cell::RefCell;

    /// Saves through `run_save_with_login`, returning the result and the settings it wrote.
    fn save(
        read_only: bool,
        draft: &Draft,
        base: &Settings,
        auto: Option<&dyn Autostart>,
        update_ok: bool,
    ) -> (SaveResult, Option<Settings>) {
        let written = RefCell::new(None);
        let r = run_save_with_login(read_only, draft, base, auto, |s| {
            *written.borrow_mut() = Some(s);
            update_ok
        });
        (r, written.into_inner())
    }

    fn want_login(on: bool) -> (Draft, Settings) {
        let base = defaults();
        let mut d = Draft::from_settings(&base);
        d.launch_at_login = on;
        (d, base)
    }

    #[test]
    fn turning_start_at_login_on_registers_it_and_saves_the_choice() {
        let fake = FakeAutostart::new(false, false);
        let (d, base) = want_login(true);
        let (r, w) = save(false, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(fake.enabled());
        assert!(w.unwrap().startup.launch_at_login);
    }

    #[test]
    fn turning_it_off_removes_the_registration() {
        let fake = FakeAutostart::new(true, false);
        let mut base = defaults();
        base.startup.launch_at_login = true;
        let mut d = Draft::from_settings(&base);
        d.launch_at_login = false;
        let (r, w) = save(false, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(!fake.enabled());
        assert!(!w.unwrap().startup.launch_at_login);
    }

    #[test]
    fn a_failed_registration_is_reported_and_the_stored_choice_matches_reality() {
        let fake = FakeAutostart::new(false, true);
        let (mut d, base) = want_login(true);
        d.shortcut = "Ctrl+Alt+K".into();
        let (r, w) = save(false, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::SavedLoginFailed));
        let w = w.unwrap();
        // Not registered, so not stored as on; the other changes are still saved.
        assert!(!w.startup.launch_at_login);
        assert_eq!(w.shortcuts.toggle_popup, "Ctrl+Alt+K");
        assert!(!fake.enabled());
    }

    #[test]
    fn an_unchanged_toggle_saves_normally() {
        let ok = FakeAutostart::new(false, false);
        let (d, base) = want_login(false);
        let (r, _) = save(false, &d, &base, Some(&ok), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(!ok.enabled());
    }

    #[test]
    fn a_platform_without_autostart_only_stores_the_choice() {
        let (d, base) = want_login(true);
        let (r, w) = save(false, &d, &base, None, true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(w.unwrap().startup.launch_at_login);
    }

    #[test]
    fn a_failed_settings_write_undoes_the_registration() {
        let fake = FakeAutostart::new(false, false);
        let (d, base) = want_login(true);
        let (r, _) = save(false, &d, &base, Some(&fake), false);
        assert!(matches!(r, SaveResult::Failed));
        assert!(!fake.enabled());
    }

    #[test]
    fn read_only_and_invalid_saves_never_touch_the_registry() {
        let fake = FakeAutostart::new(false, false);
        let (d, base) = want_login(true);
        let (r, w) = save(true, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::ReadOnly) && w.is_none());
        assert!(!fake.enabled());
        let mut bad = d.clone();
        bad.max_items = "x".into();
        let (r, w) = save(false, &bad, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::Invalid(_)) && w.is_none());
        assert!(!fake.enabled());
    }

    /// A registry that cannot be read but can be written, and remembers the last write.
    struct WriteOnly(std::sync::Mutex<Option<bool>>);
    impl Autostart for WriteOnly {
        fn is_enabled(&self) -> Result<bool, darkclip_platform::PlatformError> {
            Err(darkclip_platform::PlatformError::Backend { code: 5 })
        }
        fn set_enabled(&self, e: bool) -> Result<(), darkclip_platform::PlatformError> {
            *self.0.lock().unwrap() = Some(e);
            Ok(())
        }
    }

    #[test]
    fn rollback_restores_the_previous_state_not_the_opposite_of_the_request() {
        // Unreadable registry, stored choice "on", the form keeps "on": the write succeeds and
        // the settings write fails. The previous state was "on" (the stored choice), so that is
        // what comes back; "off" would silently remove the user's entry.
        let reg = WriteOnly(std::sync::Mutex::new(None));
        let mut base = defaults();
        base.startup.launch_at_login = true;
        let d = Draft::from_settings(&base);
        let (r, _) = save(false, &d, &base, Some(&reg), false);
        assert!(matches!(r, SaveResult::Failed));
        assert_eq!(*reg.0.lock().unwrap(), Some(true));
    }

    #[test]
    fn a_stale_form_keeps_the_welcomes_start_at_login_answer() {
        use crate::prefs::welcome::{run_welcome, WelcomeResult};
        let fake = FakeAutostart::new(false, false);
        let opened_with = defaults();
        // The settings form opens with start at login off.
        let mut d = Draft::from_settings(&opened_with);
        assert!(!d.launch_at_login);
        // While it is open, the welcome's Start turns start at login on.
        let mut after_welcome = None;
        let r = run_welcome(false, true, true, &opened_with, Some(&fake), |s| {
            after_welcome = Some(s);
            true
        });
        assert_eq!(r, WelcomeResult::Done);
        let worker = after_welcome.unwrap();
        assert!(worker.startup.launch_at_login && fake.enabled());
        // The form is saved with only the language changed.
        d.language = Lang::Vi;
        let (r, w) = save(false, &d, &worker, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        let w = w.unwrap();
        assert!(w.startup.launch_at_login, "login stays on");
        assert_eq!(w.ui.language, Lang::Vi);
        assert!(fake.enabled(), "the registration stays");
    }

    #[test]
    fn a_toggle_changed_in_the_form_is_written_over_the_workers_value() {
        // Opened off, the user turns it on: on is written.
        let base = defaults();
        let mut d = Draft::from_settings(&base);
        d.launch_at_login = true;
        assert!(d.apply_to(&base).unwrap().startup.launch_at_login);
        // Opened off, the welcome turned it on meanwhile, the user turns it on and off again:
        // the toggle shows what it was opened with, so the worker's "on" stays.
        let mut on = defaults();
        on.startup.launch_at_login = true;
        let d = Draft::from_settings(&base);
        assert!(d.apply_to(&on).unwrap().startup.launch_at_login);
        // Opened on, the user turns it off: off is written.
        let fake = FakeAutostart::new(true, false);
        let mut d = Draft::from_settings(&on);
        d.launch_at_login = false;
        let (r, w) = save(false, &d, &on, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(!w.unwrap().startup.launch_at_login);
        assert!(!fake.enabled());
    }

    #[test]
    fn revert_then_save_never_re_enables_a_disabled_entry() {
        // Stored "on", entry disabled in Task Manager. Revert refills the form from the stored
        // choice, then the live state is read again; Save without touching the toggle.
        let fake = FakeAutostart::new(false, false);
        let mut base = defaults();
        base.startup.launch_at_login = true;
        let mut d = Draft::from_settings(&base);
        let (toggle, drift) = toggle_after_live_read(d.launch_at_login, true, Some(false));
        assert!(drift);
        d.launch_at_login = toggle;
        let (r, w) = save(false, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(
            !fake.enabled(),
            "never re-enabled without an explicit change"
        );
        assert!(!w.unwrap().startup.launch_at_login);
        // A toggle the user already changed is kept; an unreadable registry changes nothing.
        assert_eq!(
            toggle_after_live_read(false, true, Some(true)),
            (false, false)
        );
        assert_eq!(toggle_after_live_read(true, true, None), (true, false));
        assert_eq!(
            toggle_after_live_read(true, true, Some(true)),
            (true, false)
        );
    }

    #[test]
    fn the_toggle_shows_the_live_state_and_notes_a_difference() {
        assert_eq!(login_view(true, Some(false)), (false, true));
        assert_eq!(login_view(false, Some(true)), (true, true));
        assert_eq!(login_view(true, Some(true)), (true, false));
        // Unreadable registry: the stored choice, no claim about drift.
        assert_eq!(login_view(true, None), (true, false));
    }

    #[test]
    fn a_stored_on_choice_with_a_disabled_entry_is_rewritten_by_save() {
        // The fake says "not enabled" (the entry is gone or disabled in Task Manager).
        let fake = FakeAutostart::new(false, false);
        let mut base = defaults();
        base.startup.launch_at_login = true;
        let d = Draft::from_settings(&base);
        let (r, w) = save(false, &d, &base, Some(&fake), true);
        assert!(matches!(r, SaveResult::Saved));
        assert!(fake.enabled());
        assert!(w.unwrap().startup.launch_at_login);
    }

    #[test]
    fn a_custom_data_dir_instance_cannot_set_start_at_login() {
        assert_eq!(login_availability(true, false), LoginAvail::Available);
        assert_eq!(login_availability(false, false), LoginAvail::NoAdapter);
        assert_eq!(login_availability(true, true), LoginAvail::CustomProfile);
        // The note names the reason; Available has none.
        assert!(LoginAvail::Available.note().is_none());
        assert_eq!(
            LoginAvail::CustomProfile.note(),
            Some(Msg::SettingsStartAtLoginCustomProfile)
        );
        assert_eq!(
            LoginAvail::NoAdapter.note(),
            Some(Msg::SettingsStartAtLoginNote)
        );
        // Without an adapter the choice is only stored (the note says so); the toggle stays.
        assert!(LoginAvail::Available.editable());
        assert!(LoginAvail::NoAdapter.editable());
        assert!(!LoginAvail::CustomProfile.editable());
    }
}
