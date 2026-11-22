# Product context

## Why
OS clipboards keep little history (Win+V: 25 items, cloud-synced via a Microsoft account) or none. Good managers are single-platform (Ditto, Maccy) or power-user oriented (CopyQ). AI agents now want clipboard context, and the clipboard carries secrets. Full reasoning: `docs/product/product-vision.md`.

## Users (assumed personas)
Developer (Windows + Linux), technical writer (macOS, images, Darkshot), privacy-conscious Linux user (KDE, KeePassXC), agent user (MCP client). See `docs/product/user-journeys.md`.

## Expected behaviour (P0)
- Shortcut (default Ctrl+Shift+V / Cmd+Shift+V) opens a popup near the cursor; typing filters; Enter copies; Shift+Enter copies as plain text; Ctrl/Cmd+Enter pastes where supported; Esc closes.
- Duplicates move to the top instead of repeating.
- Concealed, Darkshot-excluded and excluded-app content never appears.
- Pause (timed) and private mode, visible in the tray.
- Retention defaults: 1,000 items, 30 days, 500 MiB; pinned exempt.
- Detected secrets are dropped by default (best effort, never "guaranteed").
- English and Vietnamese UI; always dark.
- Wayland sessions without data-control show "capture unavailable" instead of failing silently.
