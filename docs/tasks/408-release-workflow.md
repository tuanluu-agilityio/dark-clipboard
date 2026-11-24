# Task 408: Release workflow (Windows, unsigned + SHA-256)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/408-release-workflow` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-08, GC-14, GC-18. **Plan:** Phase 5 Task 408; D-058 (owner authorized releases and public tags), D-068.

## Scope decisions (D-068)
- 0.1.0 ships **Windows x64 only**. macOS and Linux have no clipboard adapters yet (Phase 4, Q-04), so the workflow builds no macOS or Linux artifact and the release says so.
- **Unsigned**, as Darkshot did: no Authenticode certificate. Every asset gets a SHA-256 checksum, and the release notes say the binary is unsigned and how to verify the checksum.

## Requirements
1. `.github/workflows/release.yml`:
   - Triggers: push of a tag matching `v*`, and `workflow_dispatch` with an input `dry_run` (default `true`).
   - Permissions: `contents: write` only in the job that publishes; every other job is read-only.
   - Third-party actions pinned to a full commit SHA, with the tag in a comment.
   - Job `build` on `windows-latest`:
     - runs `./scripts/check.ps1` first;
     - checks the tag: `vX.Y.Z` must equal the workspace version in `Cargo.toml`; `vX.Y.Z-<pre>` (for example `-rc.1`) is allowed when `X.Y.Z` matches, and is published as a **pre-release**;
     - runs `cargo build --release --locked -p dark-clipboard`;
     - packages `dark-clipboard-<version>-windows-x64.zip` containing `dark-clipboard.exe`, `LICENSE`, `README.md`, `CHANGELOG.md`, `THIRD-PARTY-NOTICES.txt`;
     - writes `SHA256SUMS.txt` covering the zip and the bare exe, which is also attached;
     - uploads everything as a workflow artifact.
   - Job `publish`:
     - runs only for a tag push, never for a dry run;
     - creates the GitHub release with the `gh` CLI. The title is `Dark Clipboard <version>`. The body is the `CHANGELOG.md` section for that version, extracted by a script, followed by a fixed "Unsigned Windows build; verify with SHA256SUMS.txt (`Get-FileHash`)" paragraph and the platform line: Windows tested by the developer only, macOS/Linux not available.
     - The job fails if the CHANGELOG has no section for the version.
2. **Third-party notices (F407-07):**
   - Generate `THIRD-PARTY-NOTICES.txt` for the Windows target's normal dependency graph of `dark-clipboard`: each crate's name, version and licence expression, plus the licence texts.
   - Use `cargo about` (pinned version, `about.toml` and template committed) in the workflow. Add the same command to `scripts/release-notices.ps1` so it can be run locally.
   - Remove the two unused licence exceptions in `deny.toml` that Task 407 found, if `cargo deny check` stays green.
3. **Scripts:**
   - `scripts/changelog-section.ps1 <version>` prints that version's CHANGELOG section. Include tests or a self-check mode run by `check.ps1`, with a fixture CHANGELOG.
   - `scripts/package-windows.ps1` builds the zip and the checksums, so CI and the owner use the same steps.
4. **Docs:**
   - `docs/release.md`: how to cut a release, the workflow, how to verify a download, and what is not done (signing, macOS/Linux).
   - The README download section is written in Task 410; do not change it here.

## Tests and checks
- **Local:** run `scripts/package-windows.ps1` on a release build into a temp folder. Show the zip listing and that `Get-FileHash` matches `SHA256SUMS.txt`. Run the changelog script against the real CHANGELOG `[Unreleased]` section and the fixture.
- **CI:** `check.ps1` stays green on all three OS jobs. The workflow file passes `actionlint` if available; otherwise review it by hand.
- **No tag in this task.** The orchestrator runs a `workflow_dispatch` dry run after the merge, and pushes the test pre-release tag after Task 410.
- GC-19: never launch the app UI.

## Not doing
Code signing, installers (MSI or winget), macOS/Linux artifacts, auto-update.
