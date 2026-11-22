# Task 102: Windows `snapshot_meta` (formats and markers, never content)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/102-windows-snapshot-meta` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-04, GC-06, GC-07 (blocking), GC-10, GC-13. **Plan:** Task 102 row, amendments S-1, S-5; D-021.

## Read first
`docs/security/privacy-design.md` §2–3, `crates/darkclip-core/src/markers.rs` (`windows_markers`), `crates/darkclip-core/src/policy.rs` (how `markers` and `available` are used), spike answers 2, 5, 9, 10.

## Files
Create `crates/darkclip-windows/src/formats.rs` (pure mapping, compiled on every OS so its tests run everywhere); modify `src/{access.rs, win.rs}`.

## Requirements
1. **Thin OS seam.** All clipboard calls used by `snapshot_meta` go through a small internal trait (e.g. `ClipboardOs`: `open() -> Result<Guard, Busy>`, `enum_formats()`, `format_name(id)`, `register(name) -> id`, `dword_value(id) -> Option<u32>`, `size_of(id) -> Option<u64>`, `owner_source()`), with the real implementation in `win.rs` and a fake in tests. This is how tests count calls.
2. `snapshot_meta`: one `OpenClipboard` attempt (S-5: fail → `Busy`, no retry), `EnumClipboardFormats`, names via `GetClipboardFormatNameW` (standard formats get fixed names: `CF_UNICODETEXT`, `CF_DIB`, `CF_DIBV5`, `CF_HDROP`, …), close.
3. **Markers by format id (D-021):** for each known marker name, `RegisterClipboardFormatW(name)` (cache the ids) and compare with the enumerated ids; a match passes the **canonical** name to `markers::windows_markers`. For `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard` read the DWORD value (`GetClipboardData` + `GlobalLock`, exactly 4 bytes, `GlobalSize` checked). An unreadable DWORD is reported as `None` (fail closed is decided in core). Reading those DWORDs is the only `GetClipboardData` allowed in `snapshot_meta`.
4. **S-1 format list:** at most one `PlatformFormat` per `Mime`: image `PNG` (registered) > `CF_DIBV5` > `CF_DIB`; text `CF_UNICODETEXT` only (not `CF_TEXT`/`CF_OEMTEXT`); `HTML Format` → `TextHtml`; `Rich Text Format` → `TextRtf`; `CF_HDROP` → `UriList`. Other formats (including `DataObject`, `Ole Private Data`, marker formats) are listed with `mime: None` so the policy sees an OLE placeholder-only snapshot. `size_hint` from `GlobalSize` only where it needs no content read (handle size; leave `None` if unsure, and say which).
5. `meta.seq` = sequence number read while the clipboard is open; `meta.source` from the resolver.
6. Adapter never decides to skip (GC-04).

## Tests
- Pure (`formats.rs`, all OSes): `prefers_png_over_dibv5_over_dib`, `text_only_unicodetext`, `placeholders_have_no_mime`, `html_rtf_hdrop_mapping`, `marker_matched_by_id_not_name_case` (enumerated name with different case or a lookalike name registered under another id is **not** a marker; the registered id is).
- With the fake OS seam: `snapshot_never_reads_content` (Darkshot three-format set + `CF_UNICODETEXT` + `PNG`: only marker DWORD ids are passed to the data getter; count = 2 for the Darkshot set), `busy_returns_busy_once` (open fails → `Busy`, one open attempt), `darkshot_set_reports_darkshot_marker`, `exclude_format_any_data_reports_concealed`, `history_dword_zero_concealed_and_one_not`, `unreadable_dword_reported_none`.
- Real OS, read-only: `snapshot_meta_on_current_clipboard_does_not_fail` (Ok or Busy).
- `#[ignore = "writes the real clipboard"]`: `real_clipboard_write_darkshot_set_detected` (write the Darkshot format set exactly as in `darkshot-windows/src/clipboard.rs` per the spike + text; expect `DarkshotExcluded`), `real_clipboard_write_powershell_ole_phases` (spike S5). Do not run ignored tests.

## Not doing
`read`/`write` (103/104).
