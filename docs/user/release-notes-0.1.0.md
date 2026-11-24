# Dark Clipboard 0.1.0 release notes

First release. **Windows 11 x64 only, alpha, unsigned.** macOS and Linux have no app yet and nothing is claimed for them. The full list of changes and Known issues is the `[0.1.0]` section of the [CHANGELOG](../../CHANGELOG.md); the guide is [windows-alpha.md](windows-alpha.md).

## Download and check

Assets of the GitHub release: `dark-clipboard-0.1.0-windows-x64.zip` (the exe, `LICENSE`, `README.md`, `CHANGELOG.md`, `THIRD-PARTY-NOTICES.txt`), `dark-clipboard.exe` and `SHA256SUMS.txt`. Check the SHA-256 in PowerShell:

    Get-FileHash .\dark-clipboard-0.1.0-windows-x64.zip -Algorithm SHA256
    Get-Content .\SHA256SUMS.txt

The hash must equal the line for that file. The file is not code-signed, so Windows SmartScreen may warn; the checksum catches a damaged or swapped download but does not prove who built it. More: [docs/release.md](../release.md).

## What it does

Remembers what you copy (text, rich text, images, file lists) and finds it again as you type; `Ctrl+Shift+V` opens the popup; Enter copies back, Ctrl+Enter pastes. Collections, tags, notes and favorites; local text transforms and edited copies; backup, restore, export and import; storage usage and cleanup; an opt-in update check; clear messages when the history cannot be opened. English and Vietnamese.

## Privacy

No account, no cloud, no telemetry. Capture is off until you press Start in the welcome. Clips marked private by Windows or Darkshot, and clips from excluded apps, are never stored. The only network use is the opt-in update check (off by default). A backup is a plain, unencrypted zip: keep it out of synced folders.

## Read before you rely on it

- Alpha: almost every desktop UI row is untested by hand (owner manual rows in the platform checklist).
- Verified on a real clipboard: KeePassXC 2.8.0 and Darkshot 0.6.0 clips are not stored. KeePass 2, Bitwarden and 1Password are not verified.
- The search benchmark is around its 30 ms budget on a busy machine; libFuzzer was not run.
- Older `--data-dir` folders do not get the private access list; the KeePass 2 exclusion applies to new settings files only; file dialogs are Windows only.
