# Task 609: Website for 0.3.0 (translate in place)

**Agent:** `scaffolder` · **Model:** sonnet · **Branch:** `task/609-website-0.3.0` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14 (EN and VI), "Honest platform claims" (`CLAUDE.md`). **Source:** the owner asked for the release first, then the website (2026-11-28). 0.3.0 was released on 2026-11-28 (https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.3.0).

## Goal
Make `website/index.html` true for the downloadable 0.3.0, in EN and VI.

## Steps
1. **Starting texts.** Use the drafts in commit `bcdba3b` (`git show bcdba3b -- website/index.html`): `desc` and the two `<meta>` description tags, `lede`, `f2b`/`f2s`, and a feature card `t7h`/`t7p` in `#features` (it reuses `#globe` and has `grid-column:1/-1`). Re-apply them on current `main`, with these changes:
   - The feature is **available in 0.3.0**, not "next release". `t7h` is "Translate in place" / "Dịch tại chỗ".
   - `lede` names the two opt-in exceptions without "in the next release": the update check, and translate in place with your own Anthropic key.
   - `t7p` uses the reviewed wording. Keep the `<span class=\"k\">` key-cap markup as the other cards do:
     - EN: "Type, press Ctrl+Alt+T, and the selection, or the whole field when nothing is selected, is translated in place: Vietnamese into English, anything else into Vietnamese (or always one language, as you choose in Settings). It uses your own Anthropic key, is off by default and asks once before turning on. Excluded apps, private-marked and secret-looking text are never sent. Windows."
     - VI: "Gõ, bấm Ctrl+Alt+T, phần đang chọn (hoặc cả ô nếu không chọn gì) được dịch ngay tại chỗ: tiếng Việt sang tiếng Anh, còn lại sang tiếng Việt (hoặc luôn một ngôn ngữ, chọn trong Cài đặt). Dùng key Anthropic của riêng bạn, mặc định tắt và hỏi một lần trước khi bật. App bị loại trừ, nội dung có đánh dấu riêng tư và chữ giống bí mật không bao giờ được gửi. Chỉ Windows."
2. **Version and download.** Every "0.2.0" on the page (download section, status, file names, verify commands) becomes 0.3.0. The download links point at the v0.3.0 release assets, `dark-clipboard-0.3.0-windows-x64.zip`. The status section must not call translate in place tested: say it is new and not yet checked by hand.
3. **Checks.**
   - Run the two external-load greps from `.github/workflows/pages.yml`: no output.
   - The EN and VI dictionaries have identical key sets.
   - The strip and grid layouts are unchanged otherwise.
4. **Records** (CLAUDE.md: post-release records go in the next PR):
   - `memory-bank/decisions.md`: **D-098**, "0.3.0 released (D-097) from `main` e23e97e after a green `workflow_dispatch` dry run (run 46091585364); tag run 46093632211; https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.3.0 (not a pre-release; `releases/latest` = v0.3.0). `dark-clipboard-0.3.0-windows-x64.zip` SHA-256 1986c9c6829ca32d12cdd6f4d3d31b06d553db59c3fa00520a2ba1166eebf413, `dark-clipboard.exe` c42db8bf68aa6c2ccdaf9e0740f6f4d568c17e3424cd9c7295fc1245060a1fab; both verified with `sha256sum -c SHA256SUMS.txt` after download."
   - `memory-bank/progress.md`:
     - the Release 0.3.0 row becomes **released 2026-11-28 (D-098)**;
     - add the 609 row;
     - add a line "Phase 6 complete 2026-11-28 (608 APPROVED; 0.3.0 released; Q-08 owner rows still open)".
   - `memory-bank/activeContext.md`:
     - Phase 6 complete, 0.3.0 released;
     - remove the website follow-up;
     - next is the owner's choice: the Q-08 manual rows and a bug-fix release, or 0.2 planning (Paste Stack, D-037), or Phase 4 when hardware exists;
     - open follow-ups from 608: Credential Manager calls on the UI thread (`settings_host.rs`), and late worker jobs after the 5 s `APP_CALL_WAIT` (`translate/host.rs`); also F-42, F-44 (a), (c).

## Files this task may edit
`website/index.html`, `memory-bank/decisions.md`, `memory-bank/progress.md`, `memory-bank/activeContext.md`.

## Acceptance
- Every statement on the page is true for 0.3.0.
- No external loads, and the EN and VI key sets are equal.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- After the merge, the Pages workflow deploys (the orchestrator checks it).

## Known pitfalls
- **Pages** (Tasks 715–717): the workflow has a build job and a deploy job, and checks for external loads. Inline SVG only; no remote fonts or images.
- **Honest claims:** only what the matrix supports. Translate in place is Windows only and untested by hand.
