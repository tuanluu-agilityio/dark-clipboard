# ADR-0005: AI and MCP access model

- Status: **Accepted** as design for P2, 2026-11-22. Nothing is implemented in P0/P1.

## Decision

1. AI access is **off by default**. With no grant, the MCP server answers only `get_storage_stats` (counts only) and returns `PERMISSION_DENIED` for every other tool.
2. Transport is **stdio only** (`darkclip mcp serve`, spawned by the MCP client). There is no network listener, no HTTP/SSE transport in P2.
3. Grants are per `client_id` with explicit **scopes** (`clips:read-metadata`, `clips:read-content`, `collections:read`, `collections:write`, `clips:write-organization`, `clipboard:write`, `export`), optional collection filter and max age. Grants are created only in the desktop Settings UI, never by the MCP client and never by the CLI without an interactive confirmation.
4. **Client identity** over stdio cannot be authenticated cryptographically. The binding is therefore: `darkclip mcp serve --client <id>` is what the user puts in the MCP client's config. Settings shows the exact command to copy per grant. This is a usability boundary, not a security boundary, and it is documented as such. Any process running as the user could read the DB directly anyway (threat model T-08).
5. **Never exposed**: masked/sensitive clips (not even metadata beyond counts), excluded content (never stored anyway), file contents of `files` clips (paths only with `clips:read-content`), the settings file, the audit log contents of other clients, arbitrary paths. `export_collection` writes only into `<data>/exports/` and returns the path; it never takes a destination path from the client.
6. **Audit**: every MCP call writes an `OperationAudit` row without content or query text.
7. **Privileged writes** (`copy_clip_to_clipboard`) go through the running desktop app over local IPC (ADR-0006) so the single clipboard actor rule holds, and the desktop shows a toast naming the client.
8. **AI providers** (P2, optional): a `Provider` trait in `darkclip-ai`. No provider is bundled or enabled by default. Local providers (Ollama-compatible HTTP on `127.0.0.1`) come first. Any cloud provider requires per-request confirmation showing exactly what will be sent. There is no background upload of any kind.

## Alternatives

- Paste MCP style (approve an app, then it can search everything): rejected; too coarse for content that may include personal data.
- HTTP MCP transport with a bearer token: postponed; it opens a local network port and adds token management.
- Letting the MCP server read the clipboard directly: rejected; it bypasses the capture policy and the single-actor rule.

## Consequences

- Agents get less than they might want by default; the user widens scopes knowingly.
- The audit log gives users visibility, matching the product principle "AI is optional and permissioned".
