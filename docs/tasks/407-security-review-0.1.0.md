# Task 407: Security re-review before 0.1.0

**Agent:** `security-reviewer` · **Model:** opus · **Branch:** `task/407-security-review` · **Review:** orchestrator
**Plan:** Phase 5 Task 407; owner prompt phase5 §8.

## Requirements
Threat-model re-review against the built system (Windows): clipboard content leaks (logs, errors, Debug, crash output), password-manager handling, unexpected network traffic (only 412 may talk), IPC authorisation (pipe ACL, single instance), DB corruption and restore, sensitive data in backups and exports, data-dir file permissions, memory, popup latency, large images, copy-back reliability. `cargo deny check`, dependency audit (`cargo tree -d`, licences). Report in `docs/security/review-0.1.0.md` with a severity per finding; high-severity items become a fix task before any release.
