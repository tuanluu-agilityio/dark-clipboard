# Lucide icons

Vendored, **unmodified**, from one pinned release. Do not edit the SVG files; to change the set,
copy the new files from the same release (or move the whole folder to a newer release in one task).

- Project: Lucide, https://lucide.dev, https://github.com/lucide-icons/lucide
- Release: `lucide-static` **1.79.0** (npm package, tarball
  https://registry.npmjs.org/lucide-static/-/lucide-static-1.79.0.tgz), files from `package/icons/`.
- Licence: ISC (some icons derive from Feather, MIT); the upstream `LICENSE` is copied here and the
  text goes into `THIRD-PARTY-NOTICES.txt` through `about.hbs`.
- Use: `ui/kit/icon.slint` maps every file to a name with `@image-url` and tints it with `colorize`.
  A test (`tests/kit.rs`) checks that every file here is referenced by the kit and the other way round.

Icons (45): search, settings, x, pin, pin-off, star, lock, shield-check, eye-off, trash-2, copy,
clipboard-paste, type, wand-sparkles, pencil, sticky-note, tag, folder, folder-plus, layers,
file-text, link, code, image, file, clock, history, more-horizontal, check, undo-2, keyboard, pause,
play, database, hard-drive-download, upload, download, info, refresh-cw, panel-right, list-checks,
chevron-right, chevron-down, plus, calendar (Task 703), sliders-horizontal, arrow-left-right (Task 705).
