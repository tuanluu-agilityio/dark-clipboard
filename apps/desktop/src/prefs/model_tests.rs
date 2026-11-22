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
    let r = run_save(true, &d, &base, |_| {
        called = true;
        true
    });
    assert!(matches!(r, SaveResult::ReadOnly));
    assert!(!called);
    assert!(matches!(
        run_save(false, &d, &base, |_| true),
        SaveResult::Saved
    ));
    assert!(matches!(
        run_save(false, &d, &base, |_| false),
        SaveResult::Failed
    ));
    let mut bad = d.clone();
    bad.max_items = "x".into();
    assert!(matches!(
        run_save(false, &bad, &base, |_| true),
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
