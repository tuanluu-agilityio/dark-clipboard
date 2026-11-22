# Dark Clipboard

Local-first, AI-native clipboard manager for Windows, macOS and Linux, written in Rust. Second product of the Darkshot ecosystem. Always-dark UI. No account, no cloud, no telemetry.

## Start of every session

1. Read `memory-bank/activeContext.md`, then `memory-bank/progress.md`. Those two are enough to know where we are.
2. Read other memory-bank files only when the current work needs them (table in `memory-bank/README.md`).
3. Run the next task with `/next-task`. See progress and token ledger with `/status`.

Do not re-read whole plans or specs unless the task needs them.

## Rules

- **Phase discipline.** Work only on the current phase in `activeContext.md`. No P2 features (CLI, MCP, AI) before 0.1.0. Do not edit another phase's task packages except to fix a factual error, and record the fix in `decisions.md`.
- **One task, one branch, one agent.** Each task runs on `task/NNN-short-name`, implemented by one subagent from `.claude/agents/` following `docs/tasks/NNN-*.md`. The orchestrating session coordinates and reviews; it does not write the task's code.
- **Independent review.** The reviewer is always a different agent from the implementer. High-risk tasks (privacy, concurrency, storage integrity, unsafe/FFI) use `security-reviewer` or `specialist` on `opus`.
- **Models.** The orchestrator picks the model per task by complexity and risk (`opus`, `sonnet`, `haiku`) and passes it as the Agent tool's `model` parameter; frontmatter models are defaults. **Never use `fable`**: owner rule inherited from Darkshot (decision 154, 2026-11-22). Never use `haiku` for privacy, concurrency or data integrity. Record the chosen model and reason in the phase plan and the model actually used in `progress.md`.
- **Quality gate.** Before any merge: `powershell -File scripts/check.ps1` (Windows) or `bash scripts/check.sh` (macOS/Linux) prints `ALL CHECKS PASSED`.
- **Protected `main`.** Merge only via pull request: push the task branch, `gh pr create --base main --head task/NNN-...`, `gh pr merge <n> --merge --delete-branch`, then `git switch main && git pull --ff-only origin main`. Run `git push origin ...` as a standalone command. Before choosing a new task number: `git fetch --prune origin` and check `git branch -r`. (Until a GitHub remote exists, merge locally with `git merge --no-ff` and note it in `progress.md`.)
- **After every task:** update `memory-bank/progress.md` (status, tokens) and `memory-bank/activeContext.md` (next task, blockers). New decisions go to `memory-bank/decisions.md`; new patterns to `systemPatterns.md`; new dependencies to `techContext.md`.
- **Approved documents are not rewritten.** ADRs change only through a new ADR that supersedes them. Phase 0 documents in `docs/` are the baseline; later changes are recorded in `decisions.md`, and the document gets a dated "Changed by D-NNN" note.
- **Privacy is not negotiable** (`docs/plans/global-constraints.md` GC-03, GC-06, GC-07): never read content before the policy decision; never log content; never make OS or Darkshot exclusion markers configurable. Never build anything that bypasses Darkshot's clipboard exclusions.
- **Honest platform claims.** Do not claim support on a platform or compositor unless `docs/architecture/platform-compatibility.md` marks it **T** (tested). Never invent test results; report the exact command and output.
- **No network** in any crate in P0/P1. No telemetry. Do not send project data to external services.
- **User-visible strings** come from `darkclip_core::i18n` (English and Vietnamese).
- **Releases** only when the owner asks: bump versions, `CHANGELOG.md` (Added / Fixed / Known issues, newest first), README download section, tag `vX.Y.Z`, GitHub release from the CHANGELOG.
- **Language.** Code, commits and docs in English. Talk to the owner in the language of their message (Darkshot sessions use Vietnamese).
- `spikes/` is throwaway Phase 0 code. Never import or copy it into `crates/` or `apps/`; re-implement from the task package.
- Darkshot lives in `D:\2026 - Tuan\shotme` (repo `darkshot`). Do not modify it from this project.

## Key documents

| Topic | File |
|---|---|
| Vision, scope, journeys | `docs/product/*.md` |
| Architecture, crates, runtime rules | `docs/architecture/system-overview.md` |
| Platform capability matrix | `docs/architecture/platform-compatibility.md` |
| Data model, schemas, examples | `docs/architecture/data-model.md`, `docs/architecture/schemas/`, `docs/architecture/examples/` |
| Privacy, threat model | `docs/security/*.md` |
| Application API, CLI, MCP | `docs/architecture/ai-integration.md` |
| Decisions | `docs/adr/*.md`, `memory-bank/decisions.md` |
| Roadmap, phase plans, constraints | `docs/plans/*.md` |
| Task packages | `docs/tasks/NNN-*.md` |
