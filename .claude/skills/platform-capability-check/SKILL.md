---
name: platform-capability-check
description: Use before claiming, documenting or changing support for a Dark Clipboard feature on an OS or compositor (README, website, release notes, capability report, platform matrix). Prevents untested platform claims.
---

# Platform capability check

1. Open `docs/architecture/platform-compatibility.md`. Find the row and column for the capability and platform.
2. Look at the evidence letter:
   - **T** (tested): you may claim it. Cite the evidence file (spike or `docs/testing/...`) and its date.
   - **D** (documented) or **R** (reported): do not claim support. Say "expected, untested" or omit it.
   - **?**: do not claim anything.
3. Changing a cell to **T** requires raw output in `docs/testing/` or `docs/spikes/` produced on that platform, with the OS build, app versions and date. Only `platform-verifier` or a specialist with real output may change it.
4. If the code adds or removes a capability, update `Capabilities` (ADR-0004) so the UI shows the real state, and update the matrix in the same task.
5. Wayland claims are per compositor (KDE, wlroots-based, GNOME). Never write "Wayland supported" without naming the compositors that were tested.
6. Never invent or extrapolate results. A passing test on Windows 11 says nothing about Windows 10 until it has been run there.
