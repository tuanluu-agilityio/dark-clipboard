---
description: Run the next Dark Clipboard task end to end - branch, implementing agent, independent review, quality gate, merge, token ledger, memory-bank update
---

Run the next Dark Clipboard task. You are the orchestrator: you do not write the task's code yourself.

1. Read `memory-bank/activeContext.md` and `memory-bank/progress.md`. The next task is the first `pending` row of the current phase, or the one `in progress`. If "Waiting on owner" lists a blocker for it, stop and ask the owner.
2. Check that `git status` is clean. Base branch: `main`, unless `activeContext.md` names a phase branch. If the task is already in progress on a branch, switch to it and continue.
3. Set the task to `in progress` in `activeContext.md` and `progress.md`; commit both on the base branch (or on the task branch if the base is the protected `main`).
4. `git fetch --prune origin` (if a remote exists) and check `git branch -r` so the number is unused. Then `git switch -c task/NNN-short-name <base>`.
5. Choose agent and model. The agent comes from the phase plan's table. The model is your decision per task (`opus`, `sonnet`, `haiku`; **never `fable`**; never `haiku` for privacy, concurrency or data integrity). Start from the plan's Model column and raise it if the agent hits architectural trouble or three fix rounds fail. Dispatch with the Agent tool: `subagent_type` = the agent, `model` = your choice, prompt: "Implement task package `docs/tasks/NNN-....md`. You are on branch `task/NNN-...`." If the agent type is unavailable, use `general-purpose` with the chosen model and paste `.claude/agents/<name>.md` at the top of the prompt.
6. When the agent reports, record its token count. If it reports a blocker: answer it if the package or spec does, otherwise ask the owner.
7. Dispatch the reviewer named in the plan (always a different agent; for high-risk tasks a model at least as strong as the implementer's): "Review branch `task/NNN-...` against `docs/tasks/NNN-....md`." Record tokens.
8. On `CHANGES REQUIRED` with blocking items: send the list back to the implementing agent, record "fix tokens", and review again. After three rounds, stop and report to the owner.
9. Run the quality gate yourself: `powershell -File scripts/check.ps1` (or `bash scripts/check.sh`). It must print `ALL CHECKS PASSED`.
10. Update `memory-bank/progress.md` (status `done`, token columns, model actually used) and `memory-bank/activeContext.md` (next task, blockers). New decisions go to `decisions.md`, patterns to `systemPatterns.md`, dependencies to `techContext.md`. If user-visible behaviour changed: update `README.md` and `CHANGELOG.md` (Unreleased). Commit on the task branch.
11. Merge. With a GitHub remote and protected `main`: `git push origin task/NNN-...` (as a standalone command), `gh pr create --base main --head task/NNN-... --title "..." --body "..."`, `gh pr merge <n> --merge --delete-branch`, `git switch main`, `git pull --ff-only origin main`, `git branch -d task/NNN-...`. Before pushing, check that the diff contains no secrets or personal data. Without a remote: `git switch main && git merge --no-ff task/NNN-...`.
12. Report to the owner briefly, in the language of their last message: task done, check results, tokens used, manual tests they should run, the next task.
