# Task 715: Project website (English and Vietnamese) on GitHub Pages

**Agent:** `scaffolder` · **Model:** sonnet (static HTML/CSS/JS and a CI workflow; no product code, no privacy logic) · **Branch:** `task/715-website` (from `main`) · **Review:** `reviewer` (sonnet, D-077 low-risk)
**Global Constraints:** GC-03/GC-06/GC-07 (privacy claims only, no code), honest platform claims (`docs/architecture/platform-compatibility.md`). **Source:** owner request 2026-11-25 ("tạo website ... có hình minh họa ... gửi prototype"), prototype reviewed by the owner, answers: (1) add Vietnamese, (2) deploy to GitHub Pages, (3) self-host the fonts (D-092).

## Read first
- The approved prototype (single file, outside the repo): `C:\Users\Client\AppData\Local\Temp\claude\3c41828fbabd47a6\dc6bb96c-4a25-4d4c-a0ae-8505bcbfbfba\scratchpad\site\index.html`. Its sources, if you need them: `template.html`, `mock.css`, `sprite.html`, `history.html`, `settings.html` in the same folder.
- `README.md` (status, download, privacy promises, keys), `CHANGELOG.md` top entry, `docs/architecture/platform-compatibility.md` §1 and §2c.
- Vietnamese UI terms: the `(English, Vietnamese)` pairs in `crates/darkclip-core/src/i18n*` (grep for the English word).

## Requirements
1. **Files.** Create `website/index.html` from the prototype, unchanged in layout, look and wording except as below. Put CSS and JS inline as in the prototype (one HTML file), plus `website/fonts/` (step 3) and `website/.nojekyll` (empty). No build step, no npm, no framework.
2. **Vietnamese.**
   - Every visible English string of the site (nav, hero, strip, section titles and texts, feature tiles, privacy steps and ticks, keys table labels, status table, download steps, warning, footer, demo note, copy-button states, the live demo's "Search clipboard", "Pinned", "Recent", "Copied", "No clips match", filter labels and status-line labels, `aria-label`s, `<title>` and meta description) gets a Vietnamese version.
   - Mechanism: elements carry `data-i18n="key"` (and `data-i18n-attr` for attributes); one inline JS object `{en:{...}, vi:{...}}` holds both languages; the English text also stays in the HTML so the page reads correctly without JS.
   - A language switch in the top bar (two buttons "EN" / "VI", `aria-pressed`, 44 px target on touch, visible focus). Choice order: `#vi` or `#en` in the URL hash, then a stored choice (`localStorage`, in try/catch), then `navigator.language` starting with `vi`, else English. Switching sets `<html lang>`, updates the hash with `history.replaceState`, and stores the choice.
   - Vietnamese copy: natural, short, same meaning as the English, no claim the English does not make. Use the app's own Vietnamese terms for UI words (ghim, yêu thích, bộ sưu tập, chế độ riêng tư, tạm dừng, ...). Key names (`Ctrl+Shift+V`), file names, commands, version numbers and product names stay as they are.
   - The two static illustrations (history and settings windows) stay in English; under each, the caption says (in VI) that the screenshot shows the English interface and the app also has Vietnamese. The live demo's sample clips stay as they are.
3. **Self-hosted fonts (no third-party request).** Remove the Google Fonts `<link>`s and `preconnect`. Add `@font-face` rules for **Geist** (variable, weights 400-700) and **Geist Mono** (variable, 400-500), WOFF2, subsets **latin, latin-ext and vietnamese** each with the right `unicode-range`, `font-display: swap`, files in `website/fonts/`. Get the files from the official Geist release (`vercel/geist-font` on GitHub) or from the Google Fonts CSS for those families; record the source URL, version and SHA-256 of each file in `website/fonts/README.md`, and add the font licence as `website/fonts/OFL.txt` (SIL Open Font License 1.1). Vietnamese text with diacritics (ế, ộ, ữ, ở) must render in Geist, not a fallback.
4. **No external loads at all.** The page must load nothing from another origin: no fonts, scripts, styles, images, analytics or iframes from outside `website/`. Plain `<a href>` links to GitHub stay. No cookies, no tracking.
5. **Claims.** Keep every claim the prototype makes and add none. If you find a sentence that contradicts `README.md` or the platform matrix, change it to match them and list it in your report. Download links point to `https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.2.0`.
6. **SEO and sharing basics.** `<link rel="alternate" hreflang>` is not needed (one URL); add `<meta name="theme-color" content="#111318">`, an inline SVG favicon (the app icon symbol, data: URI), and Open Graph `og:title`, `og:description`, `og:type=website`, `og:url=https://tuanluu-agilityio.github.io/dark-clipboard/` (no `og:image`).
7. **GitHub Pages workflow.** New `.github/workflows/pages.yml`:
   - `on: push` to `main` with `paths: ['website/**', '.github/workflows/pages.yml']`, plus `workflow_dispatch`.
   - `permissions: contents: read, pages: write, id-token: write`; `concurrency: group: pages, cancel-in-progress: false`.
   - One job `deploy` on `ubuntu-latest`, `environment: name: github-pages, url: ${{ steps.deployment.outputs.page_url }}`, steps: `actions/checkout@v4`, `actions/configure-pages@v5`, `actions/upload-pages-artifact@v3` with `path: website`, `actions/deploy-pages@v4` (`id: deployment`).
   - Before upload, a step that fails if `website/index.html` loads anything from another origin: grep for `src=` / `href=` on `<link`, `<script`, `<img`, `<iframe`, `<source` and `url(` in CSS that start with `http`, `//`; plain `<a href="https://...">` links are allowed. Keep it a short shell step, no new tools.
   - Do not change `ci.yml` or `release.yml`.
8. **Docs.** `README.md`: one line under the title: `Website: https://tuanluu-agilityio.github.io/dark-clipboard/` (English, no other change). `CHANGELOG.md` `[Unreleased]` → Added: "Project website (English and Vietnamese), published with GitHub Pages from `website/`." `memory-bank/techContext.md`: a short "Website" entry (static, `website/`, fonts Geist/Geist Mono OFL self-hosted, Pages workflow; not part of the Rust workspace or the release).

## Files this task may edit
`website/**` (new), `.github/workflows/pages.yml` (new), `README.md` (one line), `CHANGELOG.md`, `memory-bank/techContext.md`.

## Acceptance
- Open `website/index.html` from disk: the page works in English and Vietnamese (switch, hash, reload keeps choice); with JS off it reads in English. Check with headless Edge or Chrome (`--headless=new --dump-dom` / `--screenshot` on the **file**, not the owner's screen; GC-19 forbids screen captures of the desktop, a headless render of a local file is allowed) at 1440 and 400 px width, in both languages; report what you checked. `document.documentElement.scrollWidth <= innerWidth` at 400 px (or the headless minimum width, say which).
- The external-load check of step 7 passes locally (run the same shell lines with Git Bash) and fails if you temporarily add a Google Fonts link (say that you tried it, then remove it).
- Every `data-i18n` key exists in both languages and no key is unused (a tiny inline check is fine, or a one-off node/python command in your report; do not add it to the repo).
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED` (nothing in the Rust workspace changes).

## Known pitfalls
- In the prototype, `getElementById` collides with `<symbol id>` names in the icon sprite (a `list` symbol broke the demo once): never give an element an id that is also a symbol id.
- The prototype's `.hero .head` clash with the mockup's `.popup .head` was fixed by renaming to `.intro`; keep site class names distinct from the mockup classes (`.card`, `.btn`, `.nav`, `.list`, `.status`, `.menu`, `.app`, `.chip`, `.detail`).
- The site is always dark: keep `color-scheme: dark` for every `data-theme`.
- Do not enable Pages, push, or create releases; the orchestrator enables Pages (source: GitHub Actions) after merge.
