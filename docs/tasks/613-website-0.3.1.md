# Task 613: Website for 0.3.1, and the 0.3.1 release records

**Agent:** `scaffolder` · **Model:** sonnet · **Branch:** `task/613-website-0.3.1` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14 (EN and VI), "Honest platform claims" (`CLAUDE.md`). **Source:** 0.3.1 was released on 2026-11-28 (D-099). The owner asked that the website follow each release (2026-11-28). Task 609 is the model for this task.

## Goal
Make `website/index.html` true for the downloadable 0.3.1, in EN and VI, and record the release.

## Steps
1. **Version and download.** Every "0.3.0" on the page that names the current release becomes 0.3.1:
   - the pill (around l.426);
   - `dl_sub`, `d1`, `rel_btn`, in both EN and VI and in the static HTML text;
   - the release link: `releases/tag/v0.3.1`;
   - the `Get-FileHash` command: `dark-clipboard-0.3.1-windows-x64.zip`.
   Do not change any text that says a feature *came* in 0.3.0.
2. **Status note `n_w11` (EN and VI).** Replace "Translate in place is new in 0.3.0 and has not been checked by hand yet" and its VI counterpart with:
   - EN: "Translate in place (new in 0.3.0, fixed in 0.3.1) has had first checks by hand in Notepad and in Zalo web on Chrome; other apps are not checked yet."
   - VI: "Dịch tại chỗ (mới ở 0.3.0, sửa lỗi ở 0.3.1) đã được kiểm tra thủ công bước đầu trong Notepad và Zalo web trên Chrome; các ứng dụng khác chưa được kiểm tra."
   Keep the rest of `n_w11` as it is. Do not change the platform matrix: Translate stays `untested` there, because not every row was run.
3. **Checks.**
   - Run the two external-load greps from `.github/workflows/pages.yml`: they must print nothing.
   - The EN and VI dictionaries have identical key sets.
   - `grep -n "0\.3\.0" website/index.html`: only lines that say a feature came in 0.3.0 remain.
4. **Records.** CLAUDE.md says records made after a release go in the next PR, so they go here.
   - `memory-bank/decisions.md`: add **D-100**, after D-099:
     > 0.3.1 released (D-099) from `main` 9559262 after a green `workflow_dispatch` dry run (run 46135072733); tag run 46135920302; https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.3.1 (not a pre-release; `releases/latest` = v0.3.1). `dark-clipboard-0.3.1-windows-x64.zip` SHA-256 6d1ad4c0d09858b0f53da9cce8487cb2d7ffbfd25be1c645cae1f8e666ae6f96, `dark-clipboard.exe` 5f30b6f699b7f9babcca5e5d2f0f027782c0ac1315090a00e731c2fbec11f9ee; both verified with `sha256sum -c SHA256SUMS.txt` after download.
     Use the same table columns as D-098: "Orchestrator (D-099)" and "Owner".
   - `memory-bank/progress.md`:
     - the "Release 0.3.1" row changes from `in progress` to **released 2026-11-28 (D-100)**;
     - add a 613 row after it, with the same columns as the 609 row; leave the token cells empty, because the orchestrator fills them.
   - `memory-bank/activeContext.md`:
     - In the paragraph that begins "**Owner re-test 2026-11-28 (main after 610):**", replace the sentence that begins "0.3.1 scoped review APPROVED" with:
       > **0.3.1 RELEASED 2026-11-28** (D-099, D-100): https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.3.1. Website updated in Task 613. Further releases need the owner again. Open owner rows: Chrome with Ctrl+Alt+T (expect `select_all=0`, `copy_ms` under 500, no menu), and a press in Chrome, then Esc, then a press in Notepad (nothing stored). **Next (owner's choice):** those rows, 0.2 planning (Paste Stack, D-037), or Phase 4 when hardware exists.

## Files this task may edit
`website/index.html`, `memory-bank/decisions.md`, `memory-bank/progress.md`, `memory-bank/activeContext.md`.

## Acceptance
- Every statement on the page is true for 0.3.1.
- The page loads nothing from outside, and the EN and VI key sets are equal.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- After the merge, the Pages workflow deploys. The orchestrator checks this.

## Known pitfalls
- **Pages:** use inline SVG only, with no remote fonts or images. The static HTML text and the i18n dictionaries must say the same thing.
- **Honest claims:** say only what the owner checked: Notepad and Zalo web on Chrome. Translate in place is Windows only.
