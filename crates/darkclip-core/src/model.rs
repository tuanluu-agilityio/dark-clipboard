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
    /// Lowercases `raw` and checks `[\p{L}\p{N}_-]+`, at most 40 characters.
    pub fn validate_name(raw: &str) -> Result<String, CoreError> {
        if raw.chars().count() > TAG_NAME_MAX_CHARS {
            return Err(CoreError::TooLong {
                field: "tag",
                max: TAG_NAME_MAX_CHARS,
            });
        }
        if raw.is_empty()
            || !raw
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CoreError::InvalidTag);
        }
        Ok(raw.to_lowercase())
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
