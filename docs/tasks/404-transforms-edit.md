# Task 404: Local text transforms and "save edited copy"

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/404-transforms-edit` · **Review:** `security-reviewer` (opus; a new path that creates clips outside capture)
**Global Constraints:** GC-02, GC-03, GC-06, GC-09, GC-10, GC-11, GC-14, GC-18. **Plan:** Phase 5 Task 404; owner prompt phase3 §2, §5 (D-057).

## Requirements
1. Pure, deterministic functions in `darkclip_core::transform`: trim (whole text; per-line option), normalise line endings (LF / CRLF), upper / lower case (Unicode), JSON validate + pretty (2 spaces) / minify (input ≤ 1 MiB, nesting ≤ 128), URL percent-encode / decode (UTF-8; invalid → error), Base64 encode / decode (decoded output must be valid UTF-8, cap 1 MiB), basic HTML → text (tags stripped, `script`/`style` content dropped, common entities decoded, `<br>`/`<p>`/`<li>` → newlines). Markdown conversion is **not** included (not reliable; D-057). Errors are typed and content-free. Plain text of rich clips uses the existing plain representation (no new code).
2. **"Copy as…"** in the popup (a key + menu) and the history detail pane: shows a preview of the output (capped like D-051 text display) before Copy; Copy writes through the existing copy-back path (self-write suppressed) and never stores a new clip.
3. **Edit text:** history detail pane "Edit…" for text clips opens an editor; **Save as new clip** creates a new clip through a UI-only `App` method that runs the same `darkclip_core` classify, sensitive detection, capture-kind and size limits as capture (the sensitive-handling setting applies; dropped → toast, nothing stored). The original clip never changes. Images and files are not editable.
4. Masked clips: no transforms, no edit.

## Tests (first, failing)
Table tests per transform incl. Vietnamese, emoji, CRLF, empty input, invalid JSON/URL/Base64, oversize input refused, deep JSON refused; HTML with `<script>` yields none of the script text; edit-save of a secret-shaped text (runtime-assembled, GC-09) is dropped under default settings; the original is unchanged after save; Copy as never creates a clip.
