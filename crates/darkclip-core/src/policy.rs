//! Pre-read capture policy (privacy-design §2, steps 1–7).
//!
//! Pure: no I/O, no clock, no content. The decision uses only settings, the sequence number,
//! marker formats, the source app id and format names/sizes. Content is read by the capture
//! actor only after `Decision::Read` (GC-03). No logging in this module (GC-06).

use crate::markers::{is_mandatory, PrivacyMarker};
use crate::model::{CaptureMode, ClipType, ClipboardSource, ExcludedApp, Mime, Platform};
use crate::time::Timestamp;

/// Everything the pre-read decision may look at. Holds no clipboard content.
pub struct PreReadInput<'a> {
    pub seq: u64,
    pub last_self_write_seq: Option<u64>,
    pub mode: &'a CaptureMode,
    pub now: Timestamp,
    pub capture_enabled: bool,
    pub markers: &'a [PrivacyMarker],
    pub source: Option<&'a ClipboardSource>,
    pub excluded: &'a [ExcludedApp],
    pub available: &'a [AvailableFormat],
    pub enabled_kinds: &'a [ClipType],
    pub caps: SizeCaps,
}

/// One offered format: its mapped MIME, the platform format name and its size if known.
/// Format names and sizes are not content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableFormat {
    pub mime: Mime,
    pub platform_format: String,
    pub size_hint: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeCaps {
    pub text_max_bytes: u64,
    pub image_max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
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
    /// Post-read, set by the actor (Task 010).
    Sensitive,
    /// Set by the actor.
    Busy,
}

impl SkipReason {
    /// Stable `snake_case` name (stats keys, envelopes). Independent of `Debug`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Paused => "paused",
            Self::Private => "private",
            Self::SelfWrite => "self_write",
            Self::OsConcealed => "os_concealed",
            Self::DarkshotExcluded => "darkshot_excluded",
            Self::Transient => "transient",
            Self::ExcludedApp => "excluded_app",
            Self::KindDisabled => "kind_disabled",
            Self::TooLarge => "too_large",
            Self::NothingReadable => "nothing_readable",
            Self::Sensitive => "sensitive",
            Self::Busy => "busy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPlan {
    pub formats: Vec<AvailableFormat>,
    /// Cap for every non-image format (plain, HTML, RTF, uri-list): truncated on a UTF-8 boundary.
    pub truncate_text_at: u64,
    /// Cap for every **stored** image (the encoded PNG, D-043): over-cap images are rejected,
    /// never truncated. Image input an adapter converts (for example a Windows DIB) is bounded
    /// by the decode caps in `crate::limits` instead, and the PNG it produces by this cap.
    pub image_max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Read(ReadPlan),
    Skip(SkipReason),
}

/// Windows OLE placeholders: offered by many apps, never readable content on their own.
const PLACEHOLDER_FORMATS: [&str; 2] = ["DataObject", "Ole Private Data"];

/// Steps 1–7 of privacy-design §2, first match wins.
pub fn decide_pre_read(input: &PreReadInput<'_>) -> Decision {
    match decide(input) {
        Ok(plan) => Decision::Read(plan),
        Err(reason) => Decision::Skip(reason),
    }
}

/// `decide_pre_read` without the settings that only stop *storing*: capture disabled, a
/// running pause, and the enabled kinds. Private still skips. Every other step is the same,
/// in the same order, and the plan reads text/plain only.
///
/// For the source text of translate in place (Task 605), which is handed to the requester
/// and never stored (Task 603).
pub fn decide_translate_read(input: &PreReadInput<'_>) -> Decision {
    match decide_translate(input) {
        Ok(plan) => Decision::Read(plan),
        Err(reason) => Decision::Skip(reason),
    }
}

fn decide(input: &PreReadInput<'_>) -> Result<ReadPlan, SkipReason> {
    // 1. Settings. A pause whose deadline has passed is normal mode.
    if !input.capture_enabled {
        return Err(SkipReason::Disabled);
    }
    match *input.mode {
        CaptureMode::Private => return Err(SkipReason::Private),
        CaptureMode::Paused { until } if input.now < until => return Err(SkipReason::Paused),
        CaptureMode::Paused { .. } | CaptureMode::Normal => {}
    }
    privacy_steps(input)?;
    // 6. Readable formats, then enabled kinds.
    let readable = readable(input);
    if readable.is_empty() {
        return Err(SkipReason::NothingReadable);
    }
    let enabled: Vec<&AvailableFormat> = readable
        .into_iter()
        .filter(|f| mime_enabled(f.mime, input.enabled_kinds))
        .collect();
    if enabled.is_empty() {
        return Err(SkipReason::KindDisabled);
    }
    // 7. Size caps: oversize images are dropped; text is truncated at read time.
    let formats: Vec<AvailableFormat> = enabled
        .into_iter()
        .filter(|f| !(f.mime == Mime::ImagePng && image_too_large(f, input.caps)))
        .cloned()
        .collect();
    if formats.is_empty() {
        return Err(SkipReason::TooLarge);
    }
    Ok(plan(formats, input.caps))
}

fn decide_translate(input: &PreReadInput<'_>) -> Result<ReadPlan, SkipReason> {
    // 1. Settings: only Private. Disabled and a running pause stop storing, and nothing is
    //    stored here.
    if *input.mode == CaptureMode::Private {
        return Err(SkipReason::Private);
    }
    privacy_steps(input)?;
    // 6. Readable formats: text/plain only; the enabled kinds are a storing setting.
    let formats: Vec<AvailableFormat> = readable(input)
        .into_iter()
        .filter(|f| f.mime == Mime::TextPlain)
        .cloned()
        .collect();
    if formats.is_empty() {
        return Err(SkipReason::NothingReadable);
    }
    // 7. Text is truncated at read time (no image is planned).
    Ok(plan(formats, input.caps))
}

/// Steps 2–5, shared by both decisions in the same order.
fn privacy_steps(input: &PreReadInput<'_>) -> Result<(), SkipReason> {
    // 2. Our own write coming back.
    if input.last_self_write_seq == Some(input.seq) {
        return Err(SkipReason::SelfWrite);
    }
    // 3. Mandatory markers. Not configurable (GC-07); Darkshot is reported in preference.
    if input.markers.contains(&PrivacyMarker::DarkshotExcluded) {
        return Err(SkipReason::DarkshotExcluded);
    }
    if input.markers.iter().any(is_mandatory) {
        return Err(SkipReason::OsConcealed);
    }
    // 4. Transient / auto-generated.
    if input
        .markers
        .iter()
        .any(|m| matches!(m, PrivacyMarker::Transient | PrivacyMarker::AutoGenerated))
    {
        return Err(SkipReason::Transient);
    }
    // 5. Excluded app. An unknown source cannot be matched.
    if let Some(source) = input.source {
        if input.excluded.iter().any(|e| app_matches(e, source)) {
            return Err(SkipReason::ExcludedApp);
        }
    }
    Ok(())
}

/// Offered formats that are not OLE placeholders.
fn readable<'a>(input: &PreReadInput<'a>) -> Vec<&'a AvailableFormat> {
    input
        .available
        .iter()
        .filter(|f| !is_placeholder(&f.platform_format))
        .collect()
}

fn plan(formats: Vec<AvailableFormat>, caps: SizeCaps) -> ReadPlan {
    ReadPlan {
        formats,
        truncate_text_at: caps.text_max_bytes,
        image_max_bytes: caps.image_max_bytes,
    }
}

fn is_placeholder(platform_format: &str) -> bool {
    PLACEHOLDER_FORMATS
        .iter()
        .any(|p| platform_format.eq_ignore_ascii_case(p))
}

fn image_too_large(f: &AvailableFormat, caps: SizeCaps) -> bool {
    f.size_hint.is_some_and(|s| s > caps.image_max_bytes)
}

/// Which `capture_kinds` a format can produce. Plain text is classified after the read into
/// text, URL, colour or code, and is the companion of rich text, so any of those enables it.
fn mime_enabled(mime: Mime, kinds: &[ClipType]) -> bool {
    let wanted: &[ClipType] = match mime {
        Mime::TextPlain => &[
            ClipType::Text,
            ClipType::Url,
            ClipType::Color,
            ClipType::Code,
            ClipType::RichText,
        ],
        Mime::TextHtml | Mime::TextRtf => &[ClipType::RichText],
        Mime::ImagePng => &[ClipType::Image],
        Mime::UriList => &[ClipType::Files],
    };
    wanted.iter().any(|k| kinds.contains(k))
}

fn same_platform_family(a: Platform, b: Platform) -> bool {
    let linux = |p| matches!(p, Platform::LinuxX11 | Platform::LinuxWayland);
    a == b || (linux(a) && linux(b))
}

/// `app_id` match per platform; case-insensitive on Windows (exe names), exact elsewhere.
fn app_matches(e: &ExcludedApp, source: &ClipboardSource) -> bool {
    if !same_platform_family(e.platform, source.platform) {
        return false;
    }
    match source.platform {
        Platform::Windows => e.app_id.to_lowercase() == source.app_id.to_lowercase(),
        Platform::Macos | Platform::LinuxX11 | Platform::LinuxWayland => e.app_id == source.app_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markers::{linux_markers, macos_markers, windows_markers};
    use crate::model::SourceConfidence;

    #[test]
    fn skip_reason_names_are_fixed_snake_case() {
        let expected = [
            (SkipReason::Disabled, "disabled"),
            (SkipReason::Paused, "paused"),
            (SkipReason::Private, "private"),
            (SkipReason::SelfWrite, "self_write"),
            (SkipReason::OsConcealed, "os_concealed"),
            (SkipReason::DarkshotExcluded, "darkshot_excluded"),
            (SkipReason::Transient, "transient"),
            (SkipReason::ExcludedApp, "excluded_app"),
            (SkipReason::KindDisabled, "kind_disabled"),
            (SkipReason::TooLarge, "too_large"),
            (SkipReason::NothingReadable, "nothing_readable"),
            (SkipReason::Sensitive, "sensitive"),
            (SkipReason::Busy, "busy"),
        ];
        for (reason, name) in expected {
            assert_eq!(reason.name(), name);
        }
    }

    const NOW: Timestamp = Timestamp(1_795_320_104_903);
    const ALL_KINDS: [ClipType; 7] = [
        ClipType::Text,
        ClipType::RichText,
        ClipType::Url,
        ClipType::Color,
        ClipType::Code,
        ClipType::Image,
        ClipType::Files,
    ];

    fn fmt(mime: Mime, name: &str, size: Option<u64>) -> AvailableFormat {
        AvailableFormat {
            mime,
            platform_format: name.to_string(),
            size_hint: size,
        }
    }

    fn text() -> AvailableFormat {
        fmt(Mime::TextPlain, "CF_UNICODETEXT", Some(12))
    }

    fn src(app_id: &str, platform: Platform) -> ClipboardSource {
        ClipboardSource {
            app_id: app_id.to_string(),
            app_name: None,
            platform,
            confidence: SourceConfidence::Owner,
        }
    }

    fn excl(app_id: &str, platform: Platform) -> ExcludedApp {
        ExcludedApp {
            app_id: app_id.to_string(),
            platform,
            name: None,
            builtin: false,
        }
    }

    /// Owns the data of a permissive input: normal mode, no markers, nothing excluded.
    struct Case {
        seq: u64,
        last_self_write_seq: Option<u64>,
        mode: CaptureMode,
        capture_enabled: bool,
        markers: Vec<PrivacyMarker>,
        source: Option<ClipboardSource>,
        excluded: Vec<ExcludedApp>,
        available: Vec<AvailableFormat>,
        enabled_kinds: Vec<ClipType>,
        caps: SizeCaps,
    }

    impl Case {
        fn new() -> Self {
            Self {
                seq: 42,
                last_self_write_seq: Some(41),
                mode: CaptureMode::Normal,
                capture_enabled: true,
                markers: Vec::new(),
                source: None,
                excluded: Vec::new(),
                available: vec![text()],
                enabled_kinds: ALL_KINDS.to_vec(),
                caps: SizeCaps {
                    text_max_bytes: 1024 * 1024,
                    image_max_bytes: 20 * 1024 * 1024,
                },
            }
        }

        fn decide(&self) -> Decision {
            decide_pre_read(&self.input())
        }

        fn decide_translate(&self) -> Decision {
            decide_translate_read(&self.input())
        }

        fn input(&self) -> PreReadInput<'_> {
            PreReadInput {
                seq: self.seq,
                last_self_write_seq: self.last_self_write_seq,
                mode: &self.mode,
                now: NOW,
                capture_enabled: self.capture_enabled,
                markers: &self.markers,
                source: self.source.as_ref(),
                excluded: &self.excluded,
                available: &self.available,
                enabled_kinds: &self.enabled_kinds,
                caps: self.caps,
            }
        }
    }

    fn assert_reads(d: &Decision) -> &ReadPlan {
        match d {
            Decision::Read(p) => p,
            Decision::Skip(r) => unreachable!("expected Read, got Skip({r:?})"),
        }
    }

    #[test]
    fn permissive_baseline_reads() {
        let p = Case::new().decide();
        let plan = assert_reads(&p);
        assert_eq!(plan.formats, vec![text()]);
        assert_eq!(plan.truncate_text_at, 1024 * 1024);
        assert_eq!(plan.image_max_bytes, 20 * 1024 * 1024);
    }

    #[test]
    fn plan_carries_both_caps() {
        let mut c = Case::new();
        c.caps = SizeCaps {
            text_max_bytes: 5,
            image_max_bytes: 7,
        };
        let d = c.decide();
        let plan = assert_reads(&d);
        assert_eq!(plan.truncate_text_at, 5);
        assert_eq!(plan.image_max_bytes, 7);
    }

    #[test]
    fn darkshot_set_is_excluded() {
        let mut c = Case::new();
        c.markers = windows_markers(&[
            ("CF_UNICODETEXT", None),
            ("ExcludeClipboardContentFromMonitorProcessing", Some(0)),
            ("CanIncludeInClipboardHistory", Some(0)),
            ("CanUploadToCloudClipboard", Some(0)),
        ]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::DarkshotExcluded));
    }

    #[test]
    fn darkshot_precedence_regardless_of_marker_order() {
        let mut c = Case::new();
        c.markers = vec![
            PrivacyMarker::OsConcealed {
                name: "CanIncludeInClipboardHistory",
            },
            PrivacyMarker::NoCloud,
            PrivacyMarker::DarkshotExcluded,
        ];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::DarkshotExcluded));
    }

    #[test]
    fn history_zero_is_concealed() {
        let mut c = Case::new();
        c.markers = windows_markers(&[
            ("CF_UNICODETEXT", None),
            ("CanIncludeInClipboardHistory", Some(0)),
        ]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::OsConcealed));
    }

    #[test]
    fn history_one_is_not() {
        let markers = windows_markers(&[
            ("CF_UNICODETEXT", None),
            ("CanIncludeInClipboardHistory", Some(1)),
        ]);
        assert!(markers.is_empty(), "{markers:?}");
        let mut c = Case::new();
        c.markers = markers;
        assert_reads(&c.decide());
    }

    #[test]
    fn no_cloud_alone_reads() {
        let mut c = Case::new();
        c.markers = windows_markers(&[
            ("CF_UNICODETEXT", None),
            ("CanUploadToCloudClipboard", Some(0)),
        ]);
        assert_eq!(c.markers, vec![PrivacyMarker::NoCloud]);
        assert_reads(&c.decide());
    }

    #[test]
    fn macos_concealed() {
        let mut c = Case::new();
        c.markers = macos_markers(&["public.utf8-plain-text", "org.nspasteboard.ConcealedType"]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::OsConcealed));
    }

    #[test]
    fn macos_transient() {
        let mut c = Case::new();
        c.markers = macos_markers(&["public.utf8-plain-text", "org.nspasteboard.TransientType"]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Transient));
    }

    #[test]
    fn macos_autogenerated() {
        let mut c = Case::new();
        c.markers = macos_markers(&[
            "public.utf8-plain-text",
            "org.nspasteboard.AutoGeneratedType",
        ]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Transient));
    }

    #[test]
    fn linux_kde_secret() {
        let secret: &[u8] = b"secret";
        let mut c = Case::new();
        c.markers = linux_markers(&[
            ("text/plain;charset=utf-8", None),
            ("x-kde-passwordManagerHint", Some(secret)),
        ]);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::OsConcealed));
    }

    #[test]
    fn linux_kde_other_value_ignored() {
        let accept: &[u8] = b"accept";
        let mut c = Case::new();
        c.markers = linux_markers(&[
            ("text/plain;charset=utf-8", None),
            ("x-kde-passwordManagerHint", Some(accept)),
        ]);
        assert!(c.markers.is_empty());
        assert_reads(&c.decide());
    }

    #[test]
    fn paused_before_deadline_skips() {
        let mut c = Case::new();
        c.mode = CaptureMode::Paused {
            until: Timestamp(NOW.0 + 1),
        };
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Paused));
    }

    #[test]
    fn paused_after_deadline_reads() {
        for until in [NOW, Timestamp(NOW.0 - 60_000)] {
            let mut c = Case::new();
            c.mode = CaptureMode::Paused { until };
            assert_reads(&c.decide());
        }
    }

    #[test]
    fn private_skips() {
        let mut c = Case::new();
        c.mode = CaptureMode::Private;
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Private));
    }

    #[test]
    fn disabled_skips() {
        let mut c = Case::new();
        c.capture_enabled = false;
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Disabled));
        // Disabled wins over every other mode.
        c.mode = CaptureMode::Private;
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Disabled));
    }

    #[test]
    fn self_write_skips() {
        let mut c = Case::new();
        c.last_self_write_seq = Some(c.seq);
        assert_eq!(c.decide(), Decision::Skip(SkipReason::SelfWrite));
        c.last_self_write_seq = None;
        assert_reads(&c.decide());
    }

    #[test]
    fn excluded_app_case_insensitive_windows() {
        let mut c = Case::new();
        c.source = Some(src("KeePassXC.exe", Platform::Windows));
        c.excluded = vec![excl("keepassxc.exe", Platform::Windows)];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::ExcludedApp));
    }

    #[test]
    fn excluded_app_case_sensitive_macos() {
        let mut c = Case::new();
        c.source = Some(src("org.KeePassXC.keepassxc", Platform::Macos));
        c.excluded = vec![excl("org.keepassxc.keepassxc", Platform::Macos)];
        assert_reads(&c.decide());
        c.source = Some(src("org.keepassxc.keepassxc", Platform::Macos));
        assert_eq!(c.decide(), Decision::Skip(SkipReason::ExcludedApp));
    }

    #[test]
    fn excluded_app_other_platform_does_not_match() {
        let mut c = Case::new();
        c.source = Some(src("keepassxc.exe", Platform::Macos));
        c.excluded = vec![excl("keepassxc.exe", Platform::Windows)];
        assert_reads(&c.decide());
    }

    #[test]
    fn excluded_app_linux_x11_and_wayland_share_ids() {
        let mut c = Case::new();
        c.source = Some(src("org.keepassxc.KeePassXC", Platform::LinuxX11));
        c.excluded = vec![excl("org.keepassxc.KeePassXC", Platform::LinuxWayland)];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::ExcludedApp));
    }

    #[test]
    fn unknown_source_is_not_excluded() {
        let mut c = Case::new();
        c.source = None;
        c.excluded = crate::model::PrivacyPolicy::default().excluded_apps;
        assert_reads(&c.decide());
    }

    #[test]
    fn kind_disabled_skips() {
        let mut c = Case::new();
        c.available = vec![fmt(Mime::ImagePng, "PNG", Some(1000))];
        c.enabled_kinds = ALL_KINDS
            .iter()
            .copied()
            .filter(|k| *k != ClipType::Image)
            .collect();
        assert_eq!(c.decide(), Decision::Skip(SkipReason::KindDisabled));
    }

    #[test]
    fn kind_disabled_drops_only_that_kind() {
        let mut c = Case::new();
        c.available = vec![text(), fmt(Mime::ImagePng, "PNG", Some(1000))];
        c.enabled_kinds = vec![ClipType::Text];
        let d = c.decide();
        assert_eq!(assert_reads(&d).formats, vec![text()]);
    }

    #[test]
    fn oversize_image_dropped_text_kept() {
        let mut c = Case::new();
        let big_text = fmt(Mime::TextPlain, "CF_UNICODETEXT", Some(5 * 1024 * 1024));
        c.available = vec![
            big_text.clone(),
            fmt(Mime::ImagePng, "PNG", Some(30 * 1024 * 1024)),
        ];
        let d = c.decide();
        let plan = assert_reads(&d);
        // Text over its cap is kept and truncated, not skipped.
        assert_eq!(plan.formats, vec![big_text]);
        assert_eq!(plan.truncate_text_at, 1024 * 1024);
    }

    #[test]
    fn image_at_cap_or_unknown_size_kept() {
        let mut c = Case::new();
        let at_cap = fmt(Mime::ImagePng, "PNG", Some(20 * 1024 * 1024));
        let unknown = fmt(Mime::ImagePng, "image/png", None);
        c.available = vec![at_cap.clone(), unknown.clone()];
        let d = c.decide();
        assert_eq!(assert_reads(&d).formats, vec![at_cap, unknown]);
    }

    #[test]
    fn only_oversize_image_skips_too_large() {
        let mut c = Case::new();
        c.available = vec![fmt(Mime::ImagePng, "PNG", Some(20 * 1024 * 1024 + 1))];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::TooLarge));
    }

    #[test]
    fn ole_placeholder_only_skips() {
        let mut c = Case::new();
        c.available = vec![
            fmt(Mime::TextPlain, "DataObject", None),
            fmt(Mime::TextPlain, "Ole Private Data", None),
        ];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::NothingReadable));
        c.available = Vec::new();
        assert_eq!(c.decide(), Decision::Skip(SkipReason::NothingReadable));
    }

    #[test]
    fn ole_placeholders_are_removed_from_plan() {
        let mut c = Case::new();
        c.available = vec![
            fmt(Mime::TextPlain, "DataObject", None),
            text(),
            fmt(Mime::TextPlain, "Ole Private Data", None),
        ];
        let d = c.decide();
        assert_eq!(assert_reads(&d).formats, vec![text()]);
    }

    /// Every variant, so a new marker forces this list to be revisited.
    fn all_markers() -> Vec<PrivacyMarker> {
        let all = vec![
            PrivacyMarker::OsConcealed { name: "test" },
            PrivacyMarker::DarkshotExcluded,
            PrivacyMarker::NoCloud,
            PrivacyMarker::Transient,
            PrivacyMarker::AutoGenerated,
            PrivacyMarker::PasswordManagerHint,
        ];
        for m in &all {
            match m {
                PrivacyMarker::OsConcealed { .. }
                | PrivacyMarker::DarkshotExcluded
                | PrivacyMarker::NoCloud
                | PrivacyMarker::Transient
                | PrivacyMarker::AutoGenerated
                | PrivacyMarker::PasswordManagerHint => {}
            }
        }
        all
    }

    #[test]
    fn mandatory_cannot_be_overridden() {
        let mandatory: Vec<_> = all_markers().into_iter().filter(is_mandatory).collect();
        assert_eq!(mandatory.len(), 3, "{mandatory:?}");
        for m in mandatory {
            let mut c = Case::new();
            // Everything else as permissive as the input allows.
            c.last_self_write_seq = None;
            c.source = Some(src("notepad.exe", Platform::Windows));
            c.excluded = Vec::new();
            c.enabled_kinds = ALL_KINDS.to_vec();
            c.caps = SizeCaps {
                text_max_bytes: u64::MAX,
                image_max_bytes: u64::MAX,
            };
            c.available = vec![text(), fmt(Mime::ImagePng, "PNG", Some(1))];
            c.markers = vec![PrivacyMarker::NoCloud, m];
            let d = c.decide();
            assert!(
                matches!(
                    d,
                    Decision::Skip(SkipReason::OsConcealed | SkipReason::DarkshotExcluded)
                ),
                "{m:?} -> {d:?}"
            );
        }
    }

    #[test]
    fn order_private_before_markers() {
        let mut c = Case::new();
        c.mode = CaptureMode::Private;
        c.markers = vec![PrivacyMarker::OsConcealed {
            name: "org.nspasteboard.ConcealedType",
        }];
        // No format inspection is needed to decide.
        c.available = Vec::new();
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Private));
    }

    #[test]
    fn order_of_steps() {
        // 2 before 3: self-write wins over a concealed marker.
        let mut c = Case::new();
        c.last_self_write_seq = Some(c.seq);
        c.markers = vec![PrivacyMarker::PasswordManagerHint];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::SelfWrite));
        // 3 before 4: concealed wins over transient.
        let mut c = Case::new();
        c.markers = vec![
            PrivacyMarker::Transient,
            PrivacyMarker::OsConcealed { name: "x" },
        ];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::OsConcealed));
        // 4 before 5: transient wins over excluded app.
        let mut c = Case::new();
        c.markers = vec![PrivacyMarker::AutoGenerated];
        c.source = Some(src("keepassxc.exe", Platform::Windows));
        c.excluded = vec![excl("keepassxc.exe", Platform::Windows)];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Transient));
        // 5 before 6: excluded app wins over a disabled kind.
        c.markers = Vec::new();
        c.enabled_kinds = Vec::new();
        assert_eq!(c.decide(), Decision::Skip(SkipReason::ExcludedApp));
        // 6 before 7: disabled kind wins over oversize.
        let mut c = Case::new();
        c.available = vec![fmt(Mime::ImagePng, "PNG", Some(u64::MAX))];
        c.enabled_kinds = vec![ClipType::Text];
        assert_eq!(c.decide(), Decision::Skip(SkipReason::KindDisabled));
    }

    // Task 603: the translate source read.

    #[test]
    fn translate_read_ignores_disabled_paused_and_kinds() {
        let mut c = Case::new();
        c.capture_enabled = false;
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Disabled));
        assert_eq!(assert_reads(&c.decide_translate()).formats, vec![text()]);
        let mut c = Case::new();
        c.mode = CaptureMode::Paused {
            until: Timestamp(NOW.0 + 60_000),
        };
        assert_eq!(c.decide(), Decision::Skip(SkipReason::Paused));
        assert_eq!(assert_reads(&c.decide_translate()).formats, vec![text()]);
        let mut c = Case::new();
        c.enabled_kinds = Vec::new();
        assert_eq!(c.decide(), Decision::Skip(SkipReason::KindDisabled));
        assert_eq!(assert_reads(&c.decide_translate()).formats, vec![text()]);
        // All three at once.
        c.capture_enabled = false;
        c.mode = CaptureMode::Paused {
            until: Timestamp(NOW.0 + 60_000),
        };
        let d = c.decide_translate();
        let plan = assert_reads(&d);
        assert_eq!(plan.truncate_text_at, 1024 * 1024);
        assert_eq!(plan.image_max_bytes, 20 * 1024 * 1024);
    }

    #[test]
    fn translate_read_still_skips_private_markers_transient_excluded_selfwrite() {
        type SetUp = fn(&mut Case);
        let cases: Vec<(&str, SetUp, SkipReason)> = vec![
            (
                "private",
                |c: &mut Case| c.mode = CaptureMode::Private,
                SkipReason::Private,
            ),
            (
                "self write",
                |c: &mut Case| c.last_self_write_seq = Some(c.seq),
                SkipReason::SelfWrite,
            ),
            (
                "darkshot",
                |c: &mut Case| c.markers = vec![PrivacyMarker::DarkshotExcluded],
                SkipReason::DarkshotExcluded,
            ),
            (
                "os concealed",
                |c: &mut Case| {
                    c.markers = vec![PrivacyMarker::OsConcealed { name: "x" }];
                },
                SkipReason::OsConcealed,
            ),
            (
                "password manager hint",
                |c: &mut Case| c.markers = vec![PrivacyMarker::PasswordManagerHint],
                SkipReason::OsConcealed,
            ),
            (
                "transient",
                |c: &mut Case| c.markers = vec![PrivacyMarker::Transient],
                SkipReason::Transient,
            ),
            (
                "auto generated",
                |c: &mut Case| c.markers = vec![PrivacyMarker::AutoGenerated],
                SkipReason::Transient,
            ),
            (
                "excluded app",
                |c: &mut Case| {
                    c.source = Some(src("KeePassXC.exe", Platform::Windows));
                    c.excluded = vec![excl("keepassxc.exe", Platform::Windows)];
                },
                SkipReason::ExcludedApp,
            ),
        ];
        for (what, set, reason) in cases {
            let mut c = Case::new();
            set(&mut c);
            assert_eq!(c.decide(), Decision::Skip(reason), "{what}");
            assert_eq!(c.decide_translate(), Decision::Skip(reason), "{what}");
            // Disabled, paused or no enabled kinds never hide the privacy skip.
            c.enabled_kinds = Vec::new();
            if reason != SkipReason::Private {
                c.mode = CaptureMode::Paused {
                    until: Timestamp(NOW.0 + 60_000),
                };
            }
            c.capture_enabled = false;
            assert_eq!(c.decide_translate(), Decision::Skip(reason), "{what}");
        }
    }

    #[test]
    fn translate_read_plans_text_only() {
        let png = fmt(Mime::ImagePng, "PNG", Some(1000));
        let html = fmt(Mime::TextHtml, "HTML Format", Some(100));
        let mut c = Case::new();
        c.available = vec![png.clone(), html.clone(), text()];
        let d = c.decide_translate();
        assert_eq!(assert_reads(&d).formats, vec![text()]);
        c.available = vec![png.clone()];
        assert_eq!(
            c.decide_translate(),
            Decision::Skip(SkipReason::NothingReadable)
        );
        c.available = vec![png, html];
        assert_eq!(
            c.decide_translate(),
            Decision::Skip(SkipReason::NothingReadable)
        );
        // An OLE placeholder mapped to text is still not readable text.
        c.available = vec![fmt(Mime::TextPlain, "DataObject", None)];
        assert_eq!(
            c.decide_translate(),
            Decision::Skip(SkipReason::NothingReadable)
        );
        // Text over the cap is planned (truncated at read time, which the actor refuses).
        let big = fmt(Mime::TextPlain, "CF_UNICODETEXT", Some(5 * 1024 * 1024));
        c.available = vec![big.clone()];
        let d = c.decide_translate();
        assert_eq!(assert_reads(&d).formats, vec![big]);
    }
}
