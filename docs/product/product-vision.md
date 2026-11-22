# Dark Clipboard: Product Vision

Status: Phase 0 draft, 2026-11-22. Owner approval pending.
Ecosystem: Darkshot (screenshot tool, Windows, Rust). Dark Clipboard is the second Darkshot product.

## One sentence

Dark Clipboard is a local-first clipboard history for Windows, macOS and Linux that remembers what you copy, finds it again in under a keystroke, and never stores what you or your apps marked private. AI tools can read it only when you allow it.

## Why it exists

1. Every desktop OS still has a weak clipboard. Windows' Win+V keeps 25 items and syncs through a Microsoft account. macOS 26 added a basic history, but it has no search scopes or collections. Linux depends on the desktop environment.
2. Mature managers exist (Ditto, CopyQ, Maccy), but each covers only one or two platforms, has its own privacy defaults, and none was designed for AI agents as a first-class but restricted client.
3. AI assistants now want clipboard context (Paste MCP, PowerToys Advanced Paste). The clipboard is also where passwords, tokens and private keys pass through. A clipboard manager that serves AI must be stricter about privacy than one that does not.

## Principles (ordered; earlier wins on conflict)

1. **Never capture what was marked private.** OS concealed markers, Darkshot's exclusion formats, password managers' markers, excluded apps, private mode. Sensitive-data detection is a secondary safeguard, never the primary one.
2. **Local-first.** No account, no cloud, no telemetry by default. Everything works offline.
3. **Fast and boring.** The popup appears in under 100 ms (target) and search answers while you type. Idle CPU near zero.
4. **Honest about platforms.** Each OS and compositor gets a capability matrix. We never claim support we have not tested.
5. **One Application API.** Desktop UI, CLI and MCP call the same service layer, with the same permission checks.
6. **AI is optional and permissioned.** Read-only by default, scoped, revocable, audited without content. No implicit upload to any AI provider.

## Who it is for

See `user-journeys.md` for personas. In short: developers and technical writers who copy a lot, privacy-conscious users who want history without cloud sync, and people who use AI agents and want them to use clipboard context safely.

## What success looks like (first public release)

- A user on Windows 11 installs it, presses the shortcut, finds yesterday's snippet by typing three letters, and pastes it, with no setup.
- A KeePassXC, 1Password or Bitwarden copy never appears in history (verified per app, see `platform-compatibility.md`).
- A Darkshot "copy link" or "copy image" that Darkshot marked excluded never appears in history.
- The release works without any AI provider, account or network connection.

## Differentiation (proposed; not yet validated with users)

| Opportunity | Why it is credible |
|---|---|
| Same behaviour and privacy model on three OSes | Ditto is Windows-only, Maccy macOS-only, and CopyQ's cross-platform UX is power-user oriented. |
| Privacy contract that is testable | Exclusion rules are code plus tests, with a `check` gate like Darkshot's `privacy_gate`. |
| AI access as a permission model, not a feature toggle | Scopes (read metadata, read content, write organisation, write clipboard), per-client grants, and a content-free audit log. |
| Darkshot ecosystem | Shared visual language (always-dark UI), shared engineering process, and mutual respect of exclusion metadata. |

## Non-goals for the product as a whole

- No cloud service operated by us. Sync, if ever built (P3), is end-to-end encrypted, peer-to-peer or user-owned storage, and opt-in.
- No keylogging, screen reading or input injection beyond the explicit "paste after select" action, which needs OS permission where the OS requires it.
- Not a password manager, and not a snippet/template engine in P0.
- Not a replacement for Darkshot's screenshot features.

## Assumptions (to validate)

- A1. Users accept a Windows-first public release, with macOS and Linux shipped as "preview" until their matrices are verified on real machines.
- A2. A webview-based UI (Tauri 2) meets the popup latency budget. Partly measured on Windows in Phase 0; see ADR-0001.
- A3. MCP over stdio is enough for AI integration in P2 (no network listener).
