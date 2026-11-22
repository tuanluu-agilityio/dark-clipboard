# Task 011: Application API (UI caller) and settings persistence

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/011-app-api-settings` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-02 (settings I/O lives in app, not core), GC-06, GC-10, GC-14.

## Read first
`docs/architecture/ai-integration.md` §1 (trait is normative; P1/P2 methods return `ApiError::Unsupported` for now), `docs/architecture/schemas/api-envelope.schema.json`, `docs/architecture/data-model.md` §10, Tasks 008–010 Interfaces.

## Files
Create `crates/darkclip-app/src/{api.rs, permission.rs, settings.rs, envelope.rs}`.

## Interfaces
```rust
pub enum Caller { Ui, Cli, Mcp { client_id: String } }
pub enum ApiError { NotFound, InvalidArgument(&'static str), PermissionDenied(&'static str), UnsupportedPlatform,
                    ClipboardBusy, Storage, AppNotRunning, SchemaTooNew, Internal }
impl ApiError { pub fn code(&self) -> &'static str; }            // envelope codes, exactly the schema enum
pub struct App { /* Storage (rw), CaptureHandle, Settings */ }
impl ClipApi for App { /* ai-integration §1 */ }
pub fn to_envelope<T: Serialize>(r: &Result<T, ApiError>, lang: Lang) -> serde_json::Value;
pub struct Settings { pub settings_version: u32, pub privacy: PrivacyPolicy, pub retention: RetentionPolicy,
                      pub shortcuts: Shortcuts, pub ui: UiSettings, pub startup: StartupSettings }
impl Settings { pub fn load(path: &Path) -> (Settings, LoadOutcome); pub fn save(&self, path: &Path) -> Result<(), ApiError>; }
pub enum LoadOutcome { Loaded, Defaulted, RecoveredFromCorrupt { backup: PathBuf }, LoadedWithCorrections(Vec<&'static str /* field path */>) }
```
`PermissionGuard` in `permission.rs`: in Phase 1, `Caller::Mcp` → always `PermissionDenied` except `get_storage_stats`; `Caller::Cli` → allowed (the App is in-process in Phase 1; the P2 CLI process will route mutations over IPC). Mark both rules with the comment `// P2: replaced by grant checks (ADR-0005)`.

## Tests
- `envelope_ok_matches_schema` and `envelope_error_matches_schema`: compare against `docs/architecture/examples/api-envelope.*.json` shapes (field set, `api_version`=1).
- `mcp_caller_denied_by_default` for every method except `get_storage_stats`.
- `search_hides_masked_for_non_ui_callers`: a masked clip appears for `Caller::Ui` (masked preview) and is absent for `Caller::Cli`.
- `copy_clip_routes_through_actor` (FakeHandle `current()` equals the clip's reps; `read_calls` unchanged).
- `settings_atomic_save` (temp file + rename; no partial file on simulated failure).
- `settings_corrupt_recovers` (garbage file → defaults + `.corrupt-<ts>` kept).
- `settings_os_markers_not_configurable`: JSON with `respect_os_markers: false` and a custom `max_items: 50` loads with `respect_os_markers == true`, `max_items == 50`, outcome `LoadedWithCorrections(["privacy.respect_os_markers"])`.

## Added from Task 008/009 reviews (D-025, D-026)
- `pin_clip` on a masked clip returns `InvalidArgument("masked clips cannot be pinned")` (pinning would not extend its expiry). Test `pin_masked_clip_rejected`.
- Text searches return `next_cursor: null` in the envelope; the API passes the clamped limit (MCP ≤ 50 enforced here, storage ≤ 200).

## Not doing
CLI binary, MCP server, IPC (P2).
