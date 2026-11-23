//! Domain model. JSON shapes follow `docs/architecture/schemas/`.
//! `Debug` is implemented manually for content-bearing types (GC-06).

use crate::error::CoreError;
use crate::ids::{ClipId, CollectionId, TagId};
use crate::time::Timestamp;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

pub const PREVIEW_MAX_CHARS: usize = 512;
pub const NOTE_MAX_CHARS: usize = 4000;
pub const TAG_NAME_MAX_CHARS: usize = 40;

/// The constant `schema_version: 1` carried by every versioned document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match u32::deserialize(d)? {
            1 => Ok(SchemaVersion),
            _ => Err(de::Error::custom("unsupported schema_version")),
        }
    }
}

/// Lowercase hex for 32-byte hashes.
mod hex32 {
    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn encode(b: &[u8; 32]) -> String {
        let mut s = String::with_capacity(64);
        for byte in b {
            s.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            s.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
        s
    }

    pub fn decode(s: &str) -> Option<[u8; 32]> {
        if s.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (slot, pair) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
            let hi = nibble(*pair.first()?)?;
            let lo = nibble(*pair.get(1)?)?;
            *slot = hi << 4 | lo;
        }
        Some(out)
    }

    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }

    pub fn serialize<S: Serializer>(b: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(b))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        decode(&s).ok_or_else(|| de::Error::custom("expected 64 lowercase hex characters"))
    }
}

mod hex32_opt {
    use super::hex32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(b: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match b {
            Some(b) => hex32::serialize(b, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        #[derive(Deserialize)]
        struct W(#[serde(with = "hex32")] [u8; 32]);
        Ok(Option::<W>::deserialize(d)?.map(|w| w.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipType {
    Text,
    RichText,
    Url,
    Color,
    Code,
    Image,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mime {
    #[serde(rename = "text/plain;charset=utf-8")]
    TextPlain,
    #[serde(rename = "text/html")]
    TextHtml,
    #[serde(rename = "text/rtf")]
    TextRtf,
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "text/uri-list")]
    UriList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    Inline,
    Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Representation {
    pub mime: Mime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_format: Option<String>,
    pub size_bytes: u64,
    pub storage: Storage,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex32_opt")]
    pub blob_sha256: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveCategory {
    ApiKey,
    Token,
    PrivateKey,
    Password,
    PaymentCard,
    Email,
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityLevel {
    None,
    Masked,
}

/// Never contains matched text, only category names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sensitivity {
    pub level: SensitivityLevel,
    pub categories: Vec<SensitiveCategory>,
}

impl Default for Sensitivity {
    fn default() -> Self {
        Self {
            level: SensitivityLevel::None,
            categories: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "windows")]
    Windows,
    #[serde(rename = "macos")]
    Macos,
    #[serde(rename = "linux-x11")]
    LinuxX11,
    #[serde(rename = "linux-wayland")]
    LinuxWayland,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConfidence {
    Owner,
    Frontmost,
    Declared,
}

/// Where a clip came from. Window titles are deliberately not part of the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardSource {
    pub app_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    pub platform: Platform,
    pub confidence: SourceConfidence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex32_opt")]
    pub image_thumb_sha256: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagRef {
    pub id: TagId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionRef {
    pub id: CollectionId,
    pub name: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub schema_version: SchemaVersion,
    pub id: ClipId,
    pub kind: ClipType,
    pub preview: String,
    pub representations: Vec<Representation>,
    #[serde(with = "hex32")]
    pub content_hash: [u8; 32],
    pub size_bytes: u64,
    pub created_at: Timestamp,
    pub last_copied_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<Timestamp>,
    pub copy_count: u64,
    pub use_count: u64,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ClipboardSource>,
    pub sensitivity: Sensitivity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<CollectionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
    pub metadata: ClipMetadata,
}

pub const REF_NAME_MAX_CHARS: usize = 80;

fn check_ref_names<'a>(
    field: &'static str,
    mut names: impl Iterator<Item = &'a str>,
) -> Result<(), CoreError> {
    names.try_for_each(|n| match n.chars().count() {
        0 => Err(CoreError::Invalid { field }),
        c if c > REF_NAME_MAX_CHARS => Err(CoreError::TooLong {
            field,
            max: REF_NAME_MAX_CHARS,
        }),
        _ => Ok(()),
    })
}

impl Clip {
    /// Enforces the `clip.schema.json` constraints that serde does not: preview and
    /// note length, at least one representation, `copy_count >= 1`, blob storage
    /// iff `blob_sha256`, masked clips expire, ref names 1-80 chars, non-empty `app_id`.
    /// Errors name the field only, never its value (GC-06).
    pub fn validate(&self) -> Result<(), CoreError> {
        let invalid = |field| Err(CoreError::Invalid { field });
        if self.preview.chars().count() > PREVIEW_MAX_CHARS {
            return Err(CoreError::TooLong {
                field: "preview",
                max: PREVIEW_MAX_CHARS,
            });
        }
        if self
            .note
            .as_ref()
            .is_some_and(|n| n.chars().count() > NOTE_MAX_CHARS)
        {
            return Err(CoreError::TooLong {
                field: "note",
                max: NOTE_MAX_CHARS,
            });
        }
        if self.representations.is_empty() {
            return invalid("representations");
        }
        if self.copy_count < 1 {
            return invalid("copy_count");
        }
        if self
            .representations
            .iter()
            .any(|r| (r.storage == Storage::Blob) != r.blob_sha256.is_some())
        {
            return invalid("blob_sha256");
        }
        if self.sensitivity.level == SensitivityLevel::Masked && self.expires_at.is_none() {
            return invalid("expires_at");
        }
        check_ref_names("tags", self.tags.iter().map(|t| t.name.as_str()))?;
        check_ref_names(
            "collections",
            self.collections.iter().map(|c| c.name.as_str()),
        )?;
        if self.source.as_ref().is_some_and(|s| s.app_id.is_empty()) {
            return invalid("source.app_id");
        }
        Ok(())
    }
}

/// Never prints the preview, note, source or hash (GC-06).
impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("size_bytes", &self.size_bytes)
            .field("pinned", &self.pinned)
            .field("sensitivity", &self.sensitivity)
            .field("preview", &"<redacted>")
            .field("note", &self.note.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// A user tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    pub schema_version: SchemaVersion,
    pub id: TagId,
    pub name: String,
    pub created_at: Timestamp,
}

impl Tag {
    /// Normalises a tag name: trims, Unicode-lowercases, composes to NFC, then checks
    /// `[\p{L}\p{N}_-]+` and at most 40 characters. Diacritics are kept as written
    /// (search ignores them through the FTS tokenizer). Errors never carry the input.
    pub fn validate_name(raw: &str) -> Result<String, CoreError> {
        use unicode_normalization::UnicodeNormalization;
        let name: String = raw.trim().to_lowercase().nfc().collect();
        if name.chars().count() > TAG_NAME_MAX_CHARS {
            return Err(CoreError::TooLong {
                field: "tag",
                max: TAG_NAME_MAX_CHARS,
            });
        }
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CoreError::InvalidTag);
        }
        Ok(name)
    }
}

/// Folds Vietnamese `đ`/`Đ` (U+0111/U+0110) to `d`/`D`; `None` when `s` has neither.
/// SQLite's `remove_diacritics` does not fold them (they are separate letters, not a
/// base letter plus a mark), so search indexes and queries use this copy as well.
pub fn fold_d_stroke(s: &str) -> Option<String> {
    if !s.contains(['\u{0111}', '\u{0110}']) {
        return None;
    }
    Some(
        s.chars()
            .map(|c| match c {
                '\u{0111}' => 'd',
                '\u{0110}' => 'D',
                c => c,
            })
            .collect(),
    )
}

/// Longest collection name, in characters (`collection.schema.json`).
pub const COLLECTION_NAME_MAX_CHARS: usize = 80;
/// Most tags on one clip (Task 401).
pub const TAGS_PER_CLIP_MAX: usize = 20;
/// The fixed id of the system Favorites collection, created by storage migration 2.
/// Code finds Favorites by `system_key`, never by this id.
pub const FAVORITES_COLLECTION_ID: &str = "00000000-0000-7000-8000-000000000001";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    User,
    System,
}

/// The built-in collections. Favorites is the only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCollection {
    Favorites,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionSort {
    /// Members are listed in their stored order (`collection_clips.ord`).
    Manual,
    /// Members are listed like the history (pinned first, then by recency).
    Recent,
}

/// A user or system collection (`collection.schema.json`). `clip_count` is derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub schema_version: SchemaVersion,
    pub id: CollectionId,
    pub name: String,
    pub kind: CollectionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_key: Option<SystemCollection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub clip_count: u64,
    pub sort: CollectionSort,
}

impl Collection {
    /// Trims `raw` and checks 1-80 characters. Errors name the field only.
    pub fn validate_name(raw: &str) -> Result<String, CoreError> {
        let name = raw.trim();
        match name.chars().count() {
            0 => Err(CoreError::Invalid { field: "name" }),
            n if n > COLLECTION_NAME_MAX_CHARS => Err(CoreError::TooLong {
                field: "name",
                max: COLLECTION_NAME_MAX_CHARS,
            }),
            _ => Ok(name.to_owned()),
        }
    }

    /// Checks `^#[0-9a-f]{6}$`.
    pub fn validate_color(raw: &str) -> Result<(), CoreError> {
        let ok = raw.len() == 7
            && raw.starts_with('#')
            && raw
                .bytes()
                .skip(1)
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if ok {
            Ok(())
        } else {
            Err(CoreError::Invalid { field: "color" })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub schema_version: SchemaVersion,
    pub max_items: u32,
    pub max_age_days: u32,
    pub max_storage_mb: u32,
    pub keep_pinned: bool,
    pub keep_favorites: bool,
    pub sensitive_ttl_seconds: u32,
    pub image_max_mb: u32,
    pub text_max_kb: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion,
            max_items: 1000,
            max_age_days: 30,
            max_storage_mb: 500,
            keep_pinned: true,
            keep_favorites: true,
            sensitive_ttl_seconds: 120,
            image_max_mb: 20,
            text_max_kb: 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Normal,
    Paused { until: Timestamp },
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedApp {
    pub app_id: String,
    pub platform: Platform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub builtin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveHandling {
    Drop,
    StoreMasked,
    Store,
}

/// Flat JSON form of `CaptureMode` (`mode` + `paused_until`).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeName {
    Normal,
    Paused,
    Private,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivacyPolicyRepr {
    schema_version: SchemaVersion,
    capture_enabled: bool,
    mode: ModeName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paused_until: Option<Timestamp>,
    excluded_apps: Vec<ExcludedApp>,
    capture_kinds: Vec<ClipType>,
    sensitive_handling: SensitiveHandling,
    sensitive_categories_enabled: Vec<SensitiveCategory>,
    respect_os_markers: bool,
    capture_primary_selection: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyPolicy {
    pub capture_enabled: bool,
    pub mode: CaptureMode,
    pub excluded_apps: Vec<ExcludedApp>,
    pub capture_kinds: Vec<ClipType>,
    pub sensitive_handling: SensitiveHandling,
    pub sensitive_categories_enabled: Vec<SensitiveCategory>,
    /// Always true; the mandatory marker list cannot be switched off (GC-07).
    pub respect_os_markers: bool,
    pub capture_primary_selection: bool,
}

impl From<&PrivacyPolicy> for PrivacyPolicyRepr {
    fn from(p: &PrivacyPolicy) -> Self {
        let (mode, paused_until) = match p.mode {
            CaptureMode::Normal => (ModeName::Normal, None),
            CaptureMode::Paused { until } => (ModeName::Paused, Some(until)),
            CaptureMode::Private => (ModeName::Private, None),
        };
        Self {
            schema_version: SchemaVersion,
            capture_enabled: p.capture_enabled,
            mode,
            paused_until,
            excluded_apps: p.excluded_apps.clone(),
            capture_kinds: p.capture_kinds.clone(),
            sensitive_handling: p.sensitive_handling,
            sensitive_categories_enabled: p.sensitive_categories_enabled.clone(),
            respect_os_markers: p.respect_os_markers,
            capture_primary_selection: p.capture_primary_selection,
        }
    }
}

impl Serialize for PrivacyPolicy {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PrivacyPolicyRepr::from(self).serialize(s)
    }
}

impl<'de> Deserialize<'de> for PrivacyPolicy {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = PrivacyPolicyRepr::deserialize(d)?;
        if !r.respect_os_markers {
            return Err(de::Error::custom("respect_os_markers must be true"));
        }
        let mode = match (r.mode, r.paused_until) {
            (ModeName::Normal, _) => CaptureMode::Normal,
            (ModeName::Private, _) => CaptureMode::Private,
            (ModeName::Paused, Some(until)) => CaptureMode::Paused { until },
            (ModeName::Paused, None) => {
                return Err(de::Error::custom("paused mode requires paused_until"))
            }
        };
        Ok(Self {
            capture_enabled: r.capture_enabled,
            mode,
            excluded_apps: r.excluded_apps,
            capture_kinds: r.capture_kinds,
            sensitive_handling: r.sensitive_handling,
            sensitive_categories_enabled: r.sensitive_categories_enabled,
            respect_os_markers: r.respect_os_markers,
            capture_primary_selection: r.capture_primary_selection,
        })
    }
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        let app = |id: &str, platform, name: &str| ExcludedApp {
            app_id: id.to_string(),
            platform,
            name: Some(name.to_string()),
            builtin: true,
        };
        Self {
            capture_enabled: true,
            mode: CaptureMode::Normal,
            excluded_apps: vec![
                app("keepassxc.exe", Platform::Windows, "KeePassXC"),
                app("1password.exe", Platform::Windows, "1Password"),
                app("bitwarden.exe", Platform::Windows, "Bitwarden"),
                // Belt and braces: its own markers already exclude it (privacy-design 6).
                app("darkshot.exe", Platform::Windows, "Darkshot"),
                app("com.1password.1password", Platform::Macos, "1Password"),
                app("org.keepassxc.keepassxc", Platform::Macos, "KeePassXC"),
                app(
                    "com.apple.keychainaccess",
                    Platform::Macos,
                    "Keychain Access",
                ),
                app(
                    "org.keepassxc.KeePassXC",
                    Platform::LinuxWayland,
                    "KeePassXC",
                ),
            ],
            capture_kinds: vec![
                ClipType::Text,
                ClipType::RichText,
                ClipType::Url,
                ClipType::Color,
                ClipType::Code,
                ClipType::Image,
                ClipType::Files,
            ],
            sensitive_handling: SensitiveHandling::Drop,
            sensitive_categories_enabled: vec![
                SensitiveCategory::ApiKey,
                SensitiveCategory::Token,
                SensitiveCategory::PrivateKey,
                SensitiveCategory::Password,
                SensitiveCategory::PaymentCard,
            ],
            respect_os_markers: true,
            capture_primary_selection: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP_TEXT: &str =
        include_str!("../../../docs/architecture/examples/clip.text.example.json");
    const CLIP_IMAGE: &str =
        include_str!("../../../docs/architecture/examples/clip.image.example.json");
    const CLIP_MASKED: &str =
        include_str!("../../../docs/architecture/examples/clip.masked.example.json");
    const RETENTION: &str =
        include_str!("../../../docs/architecture/examples/retention-policy.default.json");
    const PRIVACY: &str =
        include_str!("../../../docs/architecture/examples/privacy-policy.default.json");
    const PRIVACY_PAUSED: &str =
        include_str!("../../../docs/architecture/examples/privacy-policy.paused.example.json");

    #[test]
    fn clip_example_roundtrip() {
        for src in [CLIP_TEXT, CLIP_IMAGE, CLIP_MASKED] {
            let a: Clip = serde_json::from_str(src).unwrap();
            let json = serde_json::to_string(&a).unwrap();
            let b: Clip = serde_json::from_str(&json).unwrap();
            assert_eq!(a, b);
            // serialization reproduces the example document exactly
            let orig: serde_json::Value = serde_json::from_str(src).unwrap();
            assert_eq!(serde_json::to_value(&a).unwrap(), orig);
        }
    }

    #[test]
    fn defaults_match_examples() {
        let r: serde_json::Value = serde_json::from_str(RETENTION).unwrap();
        assert_eq!(serde_json::to_value(RetentionPolicy::default()).unwrap(), r);
        let p: serde_json::Value = serde_json::from_str(PRIVACY).unwrap();
        assert_eq!(serde_json::to_value(PrivacyPolicy::default()).unwrap(), p);
    }

    #[test]
    fn darkshot_is_a_builtin_exclusion_on_windows() {
        let d = PrivacyPolicy::default();
        let a = d
            .excluded_apps
            .iter()
            .find(|a| a.app_id == "darkshot.exe")
            .unwrap();
        assert!(a.builtin);
        assert_eq!(a.platform, Platform::Windows);
    }

    #[test]
    fn paused_policy_roundtrip() {
        let p: PrivacyPolicy = serde_json::from_str(PRIVACY_PAUSED).unwrap();
        assert!(matches!(p.mode, CaptureMode::Paused { .. }));
        let orig: serde_json::Value = serde_json::from_str(PRIVACY_PAUSED).unwrap();
        assert_eq!(serde_json::to_value(&p).unwrap(), orig);
    }

    #[test]
    fn policy_rejects_disabled_markers_and_paused_without_until() {
        let off = PRIVACY.replace(
            "\"respect_os_markers\": true",
            "\"respect_os_markers\": false",
        );
        assert!(serde_json::from_str::<PrivacyPolicy>(&off).is_err());
        let no_until = PRIVACY.replace("\"mode\": \"normal\"", "\"mode\": \"paused\"");
        assert!(serde_json::from_str::<PrivacyPolicy>(&no_until).is_err());
    }

    #[test]
    fn debug_never_prints_content() {
        let mut clip: Clip = serde_json::from_str(CLIP_TEXT).unwrap();
        clip.preview = "TOPSECRET-PREVIEW".to_string();
        clip.note = Some("TOPSECRET-NOTE".to_string());
        let dbg = format!("{clip:?}");
        assert!(!dbg.contains("TOPSECRET-PREVIEW"));
        assert!(!dbg.contains("TOPSECRET-NOTE"));
        let alt = format!("{clip:#?}");
        assert!(!alt.contains("TOPSECRET"));
    }

    #[test]
    fn tag_name_normalisation_trims_lowercases_and_composes() {
        // Decomposed "Tiếng" (e + U+0302 + U+0301) and uppercase, with spaces around.
        let decomposed = "  Tie\u{0302}\u{0301}ng-VIE\u{0323}\u{0302}T  ";
        let n = Tag::validate_name(decomposed).unwrap();
        assert_eq!(n, "ti\u{1ebf}ng-vi\u{1ec7}t");
        assert_eq!(n, "tiếng-việt");
        // Already composed input is unchanged, diacritics kept as written.
        assert_eq!(Tag::validate_name("Tiếng-Việt").unwrap(), "tiếng-việt");
        // Length counts composed characters: 40 decomposed pairs are 40 characters.
        assert!(Tag::validate_name(&"e\u{0301}".repeat(40)).is_ok());
        assert_eq!(Tag::validate_name("   "), Err(CoreError::InvalidTag));
        assert_eq!(Tag::validate_name("a\tb"), Err(CoreError::InvalidTag));
        // The error never carries the input.
        let e = Tag::validate_name("secret value!").unwrap_err();
        assert!(!e.to_string().contains("secret"));
        assert!(!format!("{e:?}").contains("secret"));
    }

    #[test]
    fn collection_serialises_like_the_schema() {
        let c = Collection {
            schema_version: SchemaVersion,
            id: CollectionId::parse(FAVORITES_COLLECTION_ID).unwrap(),
            name: "Favorites".into(),
            kind: CollectionKind::System,
            system_key: Some(SystemCollection::Favorites),
            color: None,
            created_at: Timestamp(1_795_320_000_000),
            updated_at: Timestamp(1_795_320_000_000),
            clip_count: 2,
            sort: CollectionSort::Recent,
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "system");
        assert_eq!(v["system_key"], "favorites");
        assert_eq!(v["sort"], "recent");
        assert!(v.get("color").is_none());
        let back: Collection = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
        for src in [
            include_str!("../../../docs/architecture/examples/collection.example.json"),
            include_str!("../../../docs/architecture/examples/collection.favorites.example.json"),
        ] {
            let a: Collection = serde_json::from_str(src).unwrap();
            let orig: serde_json::Value = serde_json::from_str(src).unwrap();
            assert_eq!(serde_json::to_value(&a).unwrap(), orig);
        }
    }

    #[test]
    fn d_stroke_folding() {
        assert_eq!(fold_d_stroke("Đà Nẵng đi"), Some("Dà Nẵng di".into()));
        assert_eq!(fold_d_stroke("tiếng việt"), None);
    }

    #[test]
    fn tag_name_rules() {
        assert_eq!(Tag::validate_name("SQL").unwrap(), "sql");
        assert_eq!(Tag::validate_name("tiếng-việt_1").unwrap(), "tiếng-việt_1");
        assert_eq!(Tag::validate_name("a b"), Err(CoreError::InvalidTag));
        assert_eq!(Tag::validate_name(""), Err(CoreError::InvalidTag));
        assert_eq!(Tag::validate_name("a!"), Err(CoreError::InvalidTag));
        assert_eq!(
            Tag::validate_name(&"a".repeat(41)),
            Err(CoreError::TooLong {
                field: "tag",
                max: 40
            })
        );
        assert!(Tag::validate_name(&"a".repeat(40)).is_ok());
        let r = Tag::validate_name(&"İ".repeat(40));
        assert!(
            matches!(
                r,
                Err(CoreError::TooLong { .. }) | Err(CoreError::InvalidTag)
            ),
            "{r:?}"
        );
    }

    #[test]
    fn clip_validate_rejects_each_violation() {
        let base: Clip = serde_json::from_str(CLIP_TEXT).unwrap();
        for src in [CLIP_TEXT, CLIP_IMAGE, CLIP_MASKED] {
            let c: Clip = serde_json::from_str(src).unwrap();
            assert_eq!(c.validate(), Ok(()));
        }
        let invalid = |field| Err(CoreError::Invalid { field });
        let check = |f: &dyn Fn(&mut Clip), want: Result<(), CoreError>| {
            let mut c = base.clone();
            f(&mut c);
            assert_eq!(c.validate(), want);
        };
        let tag = |name: &str| TagRef {
            id: TagId::parse("019a8f41-1111-7222-8333-444455556666").unwrap(),
            name: name.to_string(),
        };
        let coll = |name: &str| CollectionRef {
            id: CollectionId::parse("019a8f41-1111-7222-8333-444455556666").unwrap(),
            name: name.to_string(),
        };

        // preview <= 512 chars (counted in chars, not bytes)
        check(&|c| c.preview = "é".repeat(512), Ok(()));
        check(
            &|c| c.preview = "a".repeat(513),
            Err(CoreError::TooLong {
                field: "preview",
                max: 512,
            }),
        );
        // note <= 4000 chars
        check(&|c| c.note = Some("n".repeat(4000)), Ok(()));
        check(
            &|c| c.note = Some("n".repeat(4001)),
            Err(CoreError::TooLong {
                field: "note",
                max: 4000,
            }),
        );
        // >= 1 representation
        check(&|c| c.representations.clear(), invalid("representations"));
        // copy_count >= 1
        check(&|c| c.copy_count = 0, invalid("copy_count"));
        // blob storage => blob_sha256 (and inline => none, the schema's else branch)
        check(
            &|c| {
                c.representations[0].storage = Storage::Blob;
                c.representations[0].blob_sha256 = None;
            },
            invalid("blob_sha256"),
        );
        check(
            &|c| {
                c.representations[0].storage = Storage::Inline;
                c.representations[0].blob_sha256 = Some([7; 32]);
            },
            invalid("blob_sha256"),
        );
        // masked => expires_at
        check(
            &|c| {
                c.sensitivity.level = SensitivityLevel::Masked;
                c.expires_at = None;
            },
            invalid("expires_at"),
        );
        // ref names 1-80 chars
        check(&|c| c.tags = vec![tag(&"t".repeat(80))], Ok(()));
        check(&|c| c.tags = vec![tag("")], invalid("tags"));
        check(
            &|c| c.tags = vec![tag(&"t".repeat(81))],
            Err(CoreError::TooLong {
                field: "tags",
                max: 80,
            }),
        );
        check(&|c| c.collections = vec![coll("")], invalid("collections"));
        check(
            &|c| c.collections = vec![coll(&"c".repeat(81))],
            Err(CoreError::TooLong {
                field: "collections",
                max: 80,
            }),
        );
        // non-empty app_id
        check(
            &|c| {
                if let Some(s) = c.source.as_mut() {
                    s.app_id.clear();
                }
            },
            invalid("source.app_id"),
        );
        assert!(base.source.is_some());
    }

    #[test]
    fn hex_rejects_uppercase_and_bad_length() {
        assert!(hex32::decode(&"A".repeat(64)).is_none());
        assert!(hex32::decode(&"a".repeat(63)).is_none());
        assert_eq!(
            hex32::decode(&"ab".repeat(32)).map(|b| hex32::encode(&b)),
            Some("ab".repeat(32))
        );
    }
}
