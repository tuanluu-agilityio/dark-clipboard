# Memory bank

The project's living memory. A new session reads only `activeContext.md` and `progress.md`; the other files are read when needed.

| File | Content | Read when | Update when |
|---|---|---|---|
| `activeContext.md` | Current phase, task, branch, next step, open questions, things waiting on the owner, top risks | Every session, first | Start and end of every task |
| `progress.md` | All tasks with status, model, token ledger | Every session | End of every task |
| `projectbrief.md` | Purpose, goals, non-goals, things to avoid | Questions of scope | Scope changes |
| `productContext.md` | Why the product exists, users, expected behaviour | UI or behaviour tasks | Behaviour changes |
| `systemPatterns.md` | Architecture, traits, code conventions | Coding tasks | New pattern or trait |
| `techContext.md` | Tools, crates, build and check commands, environment | Coding tasks | New dependency or tool |
| `decisions.md` | Log of settled decisions (D-NNN) | Before proposing a change | Every new decision |

Keep each file to about one page. Details belong in `docs/` and task packages; this folder holds only what is needed to continue.
