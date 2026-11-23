# Windows alpha artifact (Task 210)

Facts about the release build that was hashed for the internal Windows alpha. "0.1.0-alpha" is the milestone name only: the binary reports version 0.0.1 (`CARGO_PKG_VERSION`, the `start` log line and Settings > About), and the version is not bumped until the owner asks for a release (D-056). This is a record, not a release: no tag was created, no GitHub release exists and the file was not uploaded anywhere.

| Item | Value |
|---|---|
| Date | 2026-11-23 |
| Commit | `576ddc7b2c1e8ca0510566b297dd5b1204d969c7` (branch `task/210-alpha-docs`; the working tree differed from this commit only in documentation files: `README.md`, `CHANGELOG.md`, `docs/`) |
| rustc | `rustc 1.100.0 (0ef1bc386 2026-11-09)` |
| Build command | `cargo build --release -p dark-clipboard` |
| File | `target\release\dark-clipboard.exe` |
| Size | 11,061,248 bytes |
| SHA-256 | `B2252329EDAE740AC7490F8D7578084A9E3B483C81D9886DB3A009F81F08C9B4` |
| Machine | Windows 11 Home Single Language 10.0.26300 x64 |

Computed with `Get-FileHash -Algorithm SHA256 target\release\dark-clipboard.exe`.

Caveats:

- The hash applies to this machine's build only. Bit-for-bit reproducible builds across machines or toolchains are not claimed or tested.
- The binary is unsigned. Windows SmartScreen may warn when it is run from another location.
- This build was not run by hand for the alpha; the behaviour checks are the owner manual rows in [platform-checklist.md](platform-checklist.md), all still untested for the desktop UI.
- Size and performance figures of earlier builds are in [perf-windows.md](perf-windows.md).
