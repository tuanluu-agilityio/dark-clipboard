//! Start-up hardening (Task 413): keep crash reports out of Windows Error Reporting (F407-01,
//! T-03) and give the data folder a private DACL (F407-02, T-04).
//!
//! The decisions are pure and tested here; the Win32 calls live in `winsys.rs`. The WER calls
//! sit behind the [`WerApi`] seam so no test ever writes the real per-user WER registry list.
//! Nothing here reads or logs clipboard data; log fields are static names only (GC-06).
#![cfg_attr(not(windows), allow(dead_code))]

use crate::log::Val;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The executable name used when the running one cannot be told.
pub const FALLBACK_EXE_NAME: &str = "dark-clipboard.exe";

/// The file name (with extension) of the running executable, as `WerAddExcludedApplication`
/// wants it; [`FALLBACK_EXE_NAME`] when there is none.
pub fn exe_file_name(exe: Option<&Path>) -> OsString {
    exe.and_then(Path::file_name)
        .map_or_else(|| OsString::from(FALLBACK_EXE_NAME), OsStr::to_os_string)
}

/// The two WER calls. `true` means the call returned `S_OK`.
pub trait WerApi {
    /// `WerAddExcludedApplication(exe_name, bAllUsers = FALSE)`: per user (HKCU), no admin.
    fn add_excluded_application(&self, exe_name: &OsStr) -> bool;
    /// `WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP)`: this process only.
    fn set_no_heap(&self) -> bool;
}

/// What the WER set-up achieved. A failure is a warning, never a reason to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WerOutcome {
    pub excluded: bool,
    pub no_heap: bool,
}

impl WerOutcome {
    pub fn ok(&self) -> bool {
        self.excluded && self.no_heap
    }

    /// Static log fields (`excluded=ok|failed no_heap=ok|failed`).
    pub fn log_fields(&self) -> Vec<(&'static str, Val)> {
        let v = |ok: bool| Val::Name(if ok { "ok" } else { "failed" });
        vec![("excluded", v(self.excluded)), ("no_heap", v(self.no_heap))]
    }
}

/// Excludes this executable from WER for the current user and drops heap memory from any
/// report of this process. Both calls are always attempted.
pub fn exclude_from_wer(api: &dyn WerApi, exe: Option<&Path>) -> WerOutcome {
    let excluded = api.add_excluded_application(&exe_file_name(exe));
    let no_heap = api.set_no_heap();
    WerOutcome { excluded, no_heap }
}

/// The real WER calls (Windows).
#[cfg(windows)]
pub struct SystemWer;

#[cfg(windows)]
impl WerApi for SystemWer {
    fn add_excluded_application(&self, exe_name: &OsStr) -> bool {
        crate::winsys::wer_add_excluded_application(exe_name)
    }

    fn set_no_heap(&self) -> bool {
        crate::winsys::wer_set_no_heap()
    }
}

/// The protected DACL of the data folder: full control for the current user and for SYSTEM,
/// inherited by files and subfolders; nothing inherited from the parent (`P`).
pub fn private_dir_sddl(user_sid: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user_sid})(A;OICI;FA;;;SY)")
}

/// The sections of an SDDL text as `(letter, value)`: `O`, `G`, `D`, `S`. Values never contain
/// a colon, so the character before every colon is a section letter.
fn sddl_sections(sddl: &str) -> Vec<(char, &str)> {
    let colons: Vec<usize> = sddl.match_indices(':').map(|(i, _)| i).collect();
    let mut out = Vec::new();
    for (n, &c) in colons.iter().enumerate() {
        let Some(letter) = c.checked_sub(1).and_then(|i| sddl.get(i..c)) else {
            continue;
        };
        let end = colons
            .get(n + 1)
            .map_or(sddl.len(), |&next| next.saturating_sub(1));
        if let (Some(ch), Some(value)) = (letter.chars().next(), sddl.get(c + 1..end)) {
            out.push((ch, value));
        }
    }
    out
}

/// True when one ACE text (without parentheses) is "allow, object and container inherit, not
/// inherited, full file access" for `sid`.
fn is_full_inheritable_allow(ace: &str, sid: &str) -> bool {
    let f: Vec<&str> = ace.split(';').collect();
    let [kind, flags, rights, object, inherit_object, who] = f.as_slice() else {
        return false;
    };
    let mut flag_set: Vec<&str> = (0..flags.len())
        .step_by(2)
        .filter_map(|i| flags.get(i..i + 2))
        .collect();
    flag_set.sort_unstable();
    *kind == "A"
        && flags.len() == 4
        && flag_set == ["CI", "OI"]
        && *rights == "FA"
        && object.is_empty()
        && inherit_object.is_empty()
        && *who == sid
}

/// True when `sddl` (as Windows writes it back, possibly with an owner part and the `AI`
/// flag) already is exactly the private DACL of [`private_dir_sddl`].
pub fn dacl_is_private(sddl: &str, user_sid: &str) -> bool {
    let Some((_, dacl)) = sddl_sections(sddl).into_iter().find(|(l, _)| *l == 'D') else {
        return false;
    };
    let (flags, mut rest) = dacl.split_at(dacl.find('(').unwrap_or(dacl.len()));
    if !flags.contains('P') {
        return false;
    }
    let mut aces = Vec::new();
    while !rest.is_empty() {
        let Some((ace, after)) = rest.strip_prefix('(').and_then(|body| body.split_once(')'))
        else {
            return false;
        };
        aces.push(ace);
        rest = after;
    }
    aces.len() == 2
        && aces.iter().any(|a| is_full_inheritable_allow(a, user_sid))
        && aces.iter().any(|a| is_full_inheritable_allow(a, "SY"))
}

/// `BUILTIN\Administrators`: the default owner of objects created by an elevated
/// administrator (or by any administrator with UAC off, as on CI runners).
pub const ADMINISTRATORS_SID: &str = "S-1-5-32-544";

/// True when a folder owned by `owner_sid` counts as owned by the current user: the user's
/// own SID, or `BUILTIN\Administrators` when the process token is an enabled member of that
/// group (an elevated run). SIDs are compared as full `S-1-...` strings, never SDDL aliases.
pub fn owner_accepted(owner_sid: &str, user_sid: &str, admin_member: bool) -> bool {
    let _ = (owner_sid, user_sid, admin_member);
    false
}

/// Only a folder Dark Clipboard created now, or one that already is its data folder (the
/// default folder or one carrying [`MARKER_FILE`]), and only when the current user owns it.
pub fn may_restrict(created_now: bool, is_data_folder: bool, owner_is_user: bool) -> bool {
    owner_is_user && (created_now || is_data_folder)
}

/// Written into a data folder that Dark Clipboard creates itself. Only a folder carrying it (or
/// the default folder) is ever given the private DACL on a later start: a `history.db` alone
/// does not count, so a user-chosen folder that holds other files is never changed (F407-02).
pub const MARKER_FILE: &str = ".dark-clipboard-data";

/// True when `dir` already is a Dark Clipboard data folder: the default one, or one carrying
/// [`MARKER_FILE`] as a plain file (a link does not count). Checked before the folder is
/// created.
pub fn is_data_folder(dir: &Path, default: Option<&Path>) -> bool {
    let _ = (dir, default);
    true
}

/// Writes [`MARKER_FILE`] into a folder Dark Clipboard has just created. `false` on failure (the
/// folder is then restricted this time only).
pub fn mark_as_ours(dir: &Path) -> bool {
    let _ = dir;
    false
}

/// Result of [`restrict_data_dir`], logged by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclOutcome {
    Applied,
    AlreadyPrivate,
    /// Not a folder Dark Clipboard created or uses: left alone.
    NotOurs,
    /// The current user does not own the folder: left alone.
    NotOwner,
    /// A Win32 call failed: a warning, the app goes on.
    Failed,
}

impl AclOutcome {
    pub fn name(self) -> &'static str {
        match self {
            AclOutcome::Applied => "applied",
            AclOutcome::AlreadyPrivate => "already_private",
            AclOutcome::NotOurs => "not_ours",
            AclOutcome::NotOwner => "not_owner",
            AclOutcome::Failed => "failed",
        }
    }

    /// Outcomes worth a warning line in the log.
    pub fn is_warning(self) -> bool {
        matches!(self, AclOutcome::NotOwner | AclOutcome::Failed)
    }
}

/// Sets the private DACL on `dir` when [`may_restrict`] allows it and it is not already set.
/// Existing children get the inherited entries (`SetNamedSecurityInfoW` propagates them).
#[cfg(windows)]
pub fn restrict_data_dir(dir: &Path, created_now: bool, is_data_folder: bool) -> AclOutcome {
    if !(created_now || is_data_folder) {
        return AclOutcome::NotOurs;
    }
    let Some(sid) = crate::single_instance::current_user_sid() else {
        return AclOutcome::Failed;
    };
    let (Some(current), Some(owner)) = (
        crate::winsys::security_sddl(dir),
        crate::winsys::owner_sid(dir),
    ) else {
        return AclOutcome::Failed;
    };
    let admin = crate::winsys::is_administrators_member();
    if !may_restrict(
        created_now,
        is_data_folder,
        owner_accepted(&owner, &sid, admin),
    ) {
        return AclOutcome::NotOwner;
    }
    // Compare in Windows' own spelling of the user (an alias for well-known accounts).
    let who = crate::winsys::sddl_spelling_of_sid(&sid).unwrap_or_else(|| sid.clone());
    if dacl_is_private(&current, &who) {
        return AclOutcome::AlreadyPrivate;
    }
    if crate::winsys::set_protected_dacl(dir, &private_dir_sddl(&sid)) {
        AclOutcome::Applied
    } else {
        AclOutcome::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

    #[test]
    fn the_exe_name_is_the_file_name_of_the_running_executable() {
        let p = PathBuf::from("Program Files")
            .join("Dark Clipboard")
            .join("dark-clipboard.exe");
        assert_eq!(
            exe_file_name(Some(&p)),
            OsString::from("dark-clipboard.exe")
        );
        let renamed = PathBuf::from("tools").join("DarkClip-0.1.0.exe");
        assert_eq!(
            exe_file_name(Some(&renamed)),
            OsString::from("DarkClip-0.1.0.exe")
        );
        assert_eq!(exe_file_name(None), OsString::from(FALLBACK_EXE_NAME));
        assert_eq!(
            exe_file_name(Some(Path::new(""))),
            OsString::from(FALLBACK_EXE_NAME)
        );
    }

    struct FakeWer {
        exclude_ok: bool,
        no_heap_ok: bool,
        calls: RefCell<Vec<String>>,
    }

    impl WerApi for FakeWer {
        fn add_excluded_application(&self, exe_name: &OsStr) -> bool {
            self.calls
                .borrow_mut()
                .push(format!("exclude:{}", exe_name.to_string_lossy()));
            self.exclude_ok
        }
        fn set_no_heap(&self) -> bool {
            self.calls.borrow_mut().push("no_heap".into());
            self.no_heap_ok
        }
    }

    fn fake(exclude_ok: bool, no_heap_ok: bool) -> FakeWer {
        FakeWer {
            exclude_ok,
            no_heap_ok,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn names(fields: &[(&'static str, Val)]) -> Vec<(&'static str, &'static str)> {
        fields
            .iter()
            .map(|(k, v)| match v {
                Val::Name(n) => (*k, *n),
                Val::Num(_) => panic!("names only"),
            })
            .collect()
    }

    #[test]
    fn wer_set_up_makes_both_calls_with_the_exe_name() {
        let api = fake(true, true);
        let exe = PathBuf::from("x").join("dark-clipboard.exe");
        let out = exclude_from_wer(&api, Some(&exe));
        assert_eq!(
            *api.calls.borrow(),
            vec!["exclude:dark-clipboard.exe".to_string(), "no_heap".into()]
        );
        assert!(out.ok());
        assert_eq!(
            names(&out.log_fields()),
            vec![("excluded", "ok"), ("no_heap", "ok")]
        );
    }

    #[test]
    fn a_wer_failure_is_reported_and_the_other_call_still_runs() {
        let api = fake(false, true);
        let out = exclude_from_wer(&api, None);
        assert_eq!(api.calls.borrow().len(), 2);
        assert_eq!(
            out,
            WerOutcome {
                excluded: false,
                no_heap: true
            }
        );
        assert!(!out.ok());
        assert_eq!(
            names(&out.log_fields()),
            vec![("excluded", "failed"), ("no_heap", "ok")]
        );
        let out = exclude_from_wer(&fake(true, false), None);
        assert!(!out.ok());
        assert_eq!(
            names(&out.log_fields()),
            vec![("excluded", "ok"), ("no_heap", "failed")]
        );
    }

    #[test]
    fn the_private_sddl_grants_the_user_and_system_only_with_inheritance() {
        assert_eq!(
            private_dir_sddl(SID),
            format!("D:P(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)")
        );
    }

    #[test]
    fn the_private_dacl_is_recognised_as_windows_writes_it_back() {
        let ok = [
            format!("D:P(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)"),
            format!("D:PAI(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)"),
            format!("D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;{SID})"),
            format!("O:{SID}D:PAI(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)"),
        ];
        for s in &ok {
            assert!(dacl_is_private(s, SID), "{s}");
        }
        let not = [
            // Not protected: the parent's entries flow in.
            format!("D:AI(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)"),
            // A third principal.
            format!("D:PAI(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),
            format!(
                "D:PAI(A;OICI;FA;;;{SID})(A;OICI;FA;;;SY)(A;OICIID;0x1200a9;;;S-1-5-21-9-9-9-1234)"
            ),
            // Inherited instead of explicit, or not inheritable.
            format!("D:PAI(A;OICIID;FA;;;{SID})(A;OICIID;FA;;;SY)"),
            format!("D:P(A;;FA;;;{SID})(A;;FA;;;SY)"),
            // Another user, or SYSTEM missing.
            "D:P(A;OICI;FA;;;S-1-5-21-1-2-3-1002)(A;OICI;FA;;;SY)".to_string(),
            format!("D:P(A;OICI;FA;;;{SID})"),
            // Less than full control, or a deny entry.
            format!("D:P(A;OICI;0x1200a9;;;{SID})(A;OICI;FA;;;SY)"),
            format!("D:P(D;OICI;FA;;;{SID})(A;OICI;FA;;;SY)"),
            // No DACL part, or garbage.
            format!("O:{SID}"),
            String::new(),
            "D:P(A;OICI;FA;;;SY".to_string(),
        ];
        for s in &not {
            assert!(!dacl_is_private(s, SID), "{s}");
        }
    }

    #[test]
    fn the_owner_is_the_user_or_administrators_for_an_elevated_run() {
        assert!(owner_accepted(SID, SID, false));
        assert!(owner_accepted(SID, SID, true));
        // Elevated (or UAC off): new folders belong to BUILTIN\Administrators.
        assert!(owner_accepted(ADMINISTRATORS_SID, SID, true));
        assert!(!owner_accepted(ADMINISTRATORS_SID, SID, false));
        // Another account, SYSTEM, or an SDDL alias instead of a SID: never.
        assert!(!owner_accepted("S-1-5-21-1-2-3-1002", SID, true));
        assert!(!owner_accepted("S-1-5-18", SID, true));
        assert!(!owner_accepted("BA", SID, true));
        assert!(!owner_accepted("", SID, true));
    }

    #[test]
    fn only_the_default_folder_or_a_marked_folder_is_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let default = tmp.path().join("DarkClipboard");
        std::fs::create_dir_all(&default).unwrap();
        assert!(is_data_folder(&default, Some(&default)));
        // A user folder that holds a history and other files, but no marker: not ours.
        let shared = tmp.path().join("Shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("history.db"), b"synthetic").unwrap();
        std::fs::write(shared.join("report.docx"), b"synthetic").unwrap();
        assert!(!is_data_folder(&shared, Some(&default)));
        assert!(!is_data_folder(&shared, None));
        // A folder Dark Clipboard created carries the marker.
        let created = tmp.path().join("created");
        std::fs::create_dir_all(&created).unwrap();
        assert!(mark_as_ours(&created));
        assert!(created.join(MARKER_FILE).is_file());
        assert!(is_data_folder(&created, Some(&default)));
        // A folder named like the marker does not count.
        let fake = tmp.path().join("fake");
        std::fs::create_dir_all(fake.join(MARKER_FILE)).unwrap();
        assert!(!is_data_folder(&fake, Some(&default)));
        // A missing folder is not ours either (it may be created later).
        assert!(!is_data_folder(&tmp.path().join("missing"), Some(&default)));
    }

    #[test]
    fn only_our_own_folder_owned_by_the_user_is_restricted() {
        assert!(may_restrict(true, false, true));
        assert!(may_restrict(false, true, true));
        assert!(may_restrict(true, true, true));
        // A folder that existed and is not a Dark Clipboard folder (say, Documents).
        assert!(!may_restrict(false, false, true));
        // Owned by someone else (an elevated run, another account).
        assert!(!may_restrict(true, true, false));
        assert!(!may_restrict(true, false, false));
    }

    #[test]
    fn acl_outcome_names_are_static_and_warnings_are_the_failures() {
        let all = [
            AclOutcome::Applied,
            AclOutcome::AlreadyPrivate,
            AclOutcome::NotOurs,
            AclOutcome::NotOwner,
            AclOutcome::Failed,
        ];
        let names: Vec<&str> = all.iter().map(|o| o.name()).collect();
        assert_eq!(
            names,
            [
                "applied",
                "already_private",
                "not_ours",
                "not_owner",
                "failed"
            ]
        );
        let warn: Vec<bool> = all.iter().map(|o| o.is_warning()).collect();
        assert_eq!(warn, [false, false, false, true, true]);
    }

    /// Real Win32 calls on a temporary folder only (never the real data folder).
    #[cfg(windows)]
    #[test]
    fn a_temp_data_folder_gets_the_private_dacl_and_its_children_inherit_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("DarkClipboardTest");
        std::fs::create_dir_all(dir.join("blobs")).unwrap();
        std::fs::write(dir.join("history.db"), b"synthetic").unwrap();
        let sid = crate::single_instance::current_user_sid().unwrap();
        // How Windows spells this user in SDDL (`LA` for the built-in Administrator).
        let who = crate::winsys::sddl_spelling_of_sid(&sid).unwrap();
        // Owned by the user, or by Administrators on an elevated run (CI runners): either is
        // accepted, so the test holds in both environments.
        let owner = crate::winsys::owner_sid(&dir).unwrap();
        assert!(
            owner_accepted(&owner, &sid, crate::winsys::is_administrators_member()),
            "{owner}"
        );

        assert_eq!(restrict_data_dir(&dir, true, false), AclOutcome::Applied);
        let sddl = crate::winsys::security_sddl(&dir).unwrap();
        assert!(dacl_is_private(&sddl, &who), "{sddl}");
        // Children carry only the two inherited entries.
        for child in [dir.join("history.db"), dir.join("blobs")] {
            let c = crate::winsys::security_sddl(&child).unwrap();
            let dacl = &c[c.find("D:").unwrap()..];
            assert_eq!(dacl.matches('(').count(), 2, "{c}");
            assert!(dacl.contains(&format!(";{who})")), "{c}");
            assert!(dacl.contains(";SY)"), "{c}");
            assert!(dacl.matches("ID;").count() == 2, "{c}");
        }
        // Idempotent: the second start leaves it alone.
        assert_eq!(
            restrict_data_dir(&dir, false, true),
            AclOutcome::AlreadyPrivate
        );
        // The user keeps full control (the temp folder can still be removed).
        std::fs::write(dir.join("new.txt"), b"x").unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_that_is_not_ours_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("SomeFolder");
        std::fs::create_dir_all(&dir).unwrap();
        let before = crate::winsys::security_sddl(&dir).unwrap();
        assert_eq!(restrict_data_dir(&dir, false, false), AclOutcome::NotOurs);
        assert_eq!(crate::winsys::security_sddl(&dir).unwrap(), before);
    }
}
