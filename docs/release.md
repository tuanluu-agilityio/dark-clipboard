# Releasing Dark Clipboard

Decisions: D-058 (the owner authorizes releases and public tags), D-068 (0.1.0 plan). The workflow is `.github/workflows/release.yml`.

## What a release is

- **Windows x64 only.** macOS and Linux have no clipboard adapters yet (Phase 4), so no macOS or Linux artifact is built, and the release notes say so.
- **Unsigned.** There is no Authenticode certificate. Windows SmartScreen may warn. Every asset has a SHA-256 checksum in `SHA256SUMS.txt`.
- Assets: `dark-clipboard-<version>-windows-x64.zip` (`dark-clipboard.exe`, `LICENSE`, `README.md`, `CHANGELOG.md`, `THIRD-PARTY-NOTICES.txt`), the bare `dark-clipboard.exe`, and `SHA256SUMS.txt` (covers the zip and the bare exe).

## How to cut a release

1. Set the workspace `version` in `Cargo.toml` and move the `[Unreleased]` entries of `CHANGELOG.md` under a new `## [X.Y.Z] - date` heading. The release notes are that section.
2. Merge to `main` through a pull request with green CI.
3. Optional dry run: GitHub > Actions > Release > Run workflow (`dry_run` is `true` by default). It runs the quality gate, builds, packages and uploads the files as a workflow artifact. It never publishes.
4. Tag and push (the tag commit must be on `main`; the workflow fails otherwise): `git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow then builds, packages and creates the GitHub release with the `gh` CLI. The title is `Dark Clipboard X.Y.Z`.

Control the tags with a repository tag ruleset for `v*` (GitHub > Settings > Rules > Rulesets): restrict who can create, update and delete `v*` tags. That ruleset is the real control; the workflow's on-`main` check is a second line. The owner creates it, it is not created by any script.

Tag rules (`scripts/check-release-tag.ps1`): `vX.Y.Z` must equal the workspace version. `vX.Y.Z-<pre>` (for example `v0.1.0-rc.1`) is allowed when `X.Y.Z` matches the workspace version, and is published as a **pre-release**. A pre-release uses the CHANGELOG section of its own full version if there is one, otherwise the section of `X.Y.Z`. The job fails when neither exists.

## Third-party notices

`THIRD-PARTY-NOTICES.txt` is **generated at release time and not committed**: it is about 300 KB and changes with every `Cargo.lock` update, so a committed copy would go stale. `cargo-about` (pinned to 0.8.4, config in `about.toml`, template `about.hbs`) covers the Windows x64 normal dependencies of `dark-clipboard` (build and dev dependencies are not shipped) and includes the `webpki-roots` CDLA-Permissive-2.0 text. `scripts/release-notices.ps1` runs the same command locally and fails if the file lacks `webpki-roots`, `CDLA-Permissive-2.0` or `GPL-3.0-only`. If `cargo-about` is installed, `scripts/check.ps1` also runs it.

## Building the package locally

```powershell
cargo build --release --locked -p dark-clipboard
scripts/package-windows.ps1 -OutDir $env:TEMP\dc-pkg
```

CI uses the same script. The script never writes into the repository.

## Verifying a download

Download `SHA256SUMS.txt` next to the file, then in PowerShell:

```powershell
Get-FileHash .\dark-clipboard-X.Y.Z-windows-x64.zip -Algorithm SHA256
Get-Content .\SHA256SUMS.txt
```

One-liners: `(Get-FileHash .\<file> -Algorithm SHA256).Hash -eq '<hash from SHA256SUMS.txt>'` in PowerShell (case-insensitive, prints `True`), or `sha256sum -c SHA256SUMS.txt` in a shell that has the files next to it. The hash must equal the line for that file. The checksum protects against a damaged or swapped download from another place; it does not prove who built it, because the build is unsigned.

## If publishing fails half way

`gh release create` can leave a partial or draft release behind. Open GitHub > Releases, delete that release (the tag stays), or run `gh release delete vX.Y.Z` (add `--cleanup-tag` only if the tag itself is wrong). Then fix the cause and re-run the failed workflow run, or push a corrected tag.

The dry run (manual run) also builds `release-notes.md` and uploads it with the artifacts; a missing CHANGELOG section is only a warning there and an error on a tag push.

## Not done

- Code signing (Authenticode), installers (MSI, winget), auto-update.
- macOS and Linux artifacts.
- The release workflow has not run yet; the first run is the dry run after merge, then the test pre-release tag.
