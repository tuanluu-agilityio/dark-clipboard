#!/usr/bin/env bash
# Quality gate for macOS/Linux (twin of scripts/check.ps1; keep in sync).
# Prints "ALL CHECKS PASSED" and exits 0 when everything holds.
set -u
cd "$(dirname "$0")/.."

fail() { echo "CHECK FAILED: $1"; exit 1; }
step() { local name="$1"; shift; echo "== $name"; "$@" || fail "$name"; }

has_ws=0; [ -f Cargo.toml ] && has_ws=1

if [ "$has_ws" = 1 ]; then
  step "cargo fmt --check" cargo fmt --all --check
  step "cargo clippy" cargo clippy --workspace --all-targets -- -D warnings
  step "cargo test" cargo test --workspace
  echo "== portable crates must not depend on OS/UI/async crates (GC-01)"
  for c in darkclip-core darkclip-platform darkclip-storage darkclip-app; do
    [ -f "crates/$c/Cargo.toml" ] || continue
    tree="$(cargo tree -p "$c" -e normal --prefix none)" || fail "cargo tree -p $c"
    banned="$(printf '%s\n' "$tree" | grep -E '^(windows|windows-sys|objc2[A-Za-z0-9_-]*|core-foundation[A-Za-z0-9_-]*|x11rb|wayland-[A-Za-z0-9_-]+|gtk[A-Za-z0-9_-]*|slint[A-Za-z0-9_-]*|tauri[A-Za-z0-9_-]*|tokio) v' | sort -u)"
    [ -z "$banned" ] || { echo "CHECK FAILED: $c depends on a banned crate:"; echo "$banned"; exit 1; }
  done
  # GC-08 as amended by D-057: the only network code is the opt-in update check in apps/desktop
  # (src/update.rs). Nothing under crates/ may depend on an HTTP or TLS client, on any target.
  echo "== crates/ stay network-free (GC-08, D-057)"
  for m in crates/*/Cargo.toml; do
    [ -f "$m" ] || continue
    tree="$(cargo tree --manifest-path "$m" -e normal,build,dev --target all --prefix none)" || fail "cargo tree --manifest-path $m"
    net="$(printf '%s\n' "$tree" | grep -E '^(ureq[A-Za-z0-9_-]*|reqwest|hyper[A-Za-z0-9_-]*|isahc|surf|attohttpc|curl[A-Za-z0-9_-]*|minreq|h2|rustls[A-Za-z0-9_-]*|tokio-rustls|native-tls|openssl[A-Za-z0-9_-]*|webpki[A-Za-z0-9_-]*) v' | sort -u)"
    [ -z "$net" ] || { echo "CHECK FAILED: $m depends on a network crate:"; echo "$net"; exit 1; }
  done
fi

rs_files() { find crates apps -name '*.rs' -not -path '*/target/*' 2>/dev/null; }

echo "== no logging in privacy-critical modules (GC-06)"
# Also the platform adapters and the app's image decoder (Task 110).
hits="$(rs_files | grep -E '^(crates/darkclip-app/src/capture/.*\.rs|crates/darkclip-app/src/image\.rs|crates/darkclip-core/src/policy\.rs|crates/darkclip-core/src/sensitive/.*\.rs|crates/darkclip-(windows|macos|linux)/src/.*\.rs)$' \
  | xargs -r grep -nE '\b(e?println|e?print|dbg)!|\b(log|tracing)::(trace|debug|info|warn|error)!|\b(trace|debug|info|warn|error)!\s*\(' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: logging macro:"; echo "$hits"; exit 1; }

echo "== headless binary: log/print lines carry no content-like words (S-10)"
# Twin of the rule in check.ps1: line based; keep such macro calls on one line.
hits="$(find apps -path '*/src/*' -name '*.rs' -not -path '*/target/*' 2>/dev/null \
  | xargs -r grep -nE '\b(e?println|e?print|dbg)!|\b(log|tracing)::(trace|debug|info|warn|error)!|\b(trace|debug|info|warn|error)!\s*\(|\.event\s*\(|\bVal::' \
  | grep -iE 'preview|content|bytes|text|query|\{[^}]*:#?\?\}' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: content-like word in log/print line:"; echo "$hits"; exit 1; }

echo "== clipboard read/write only from the capture actor (GC-03)"
hits="$(rs_files | grep -vE '^(crates/darkclip-app/src/capture/.*|crates/darkclip-platform/src/.*|crates/darkclip-(windows|macos|linux)/src/.*)$' \
  | xargs -r grep -nE '\b(access|acc|clipboard_access)\s*\.\s*(read|write)\s*\(' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: clipboard access outside capture actor:"; echo "$hits"; exit 1; }
# Hardening (Task 010 review): ClipboardAccess type or `.access` field outside the allowed files; comment lines ignored.
hits="$(rs_files | grep -vE '^(crates/darkclip-app/src/capture/.*|crates/darkclip-platform/src/.*|crates/darkclip-(windows|macos|linux)/src/.*)$' \
  | xargs -r grep -nE '\bClipboardAccess\b|\.access\b|\baccess[[:space:]]*:' | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: ClipboardAccess used outside capture actor:"; echo "$hits"; exit 1; }

echo "== no renamed network dependency or Windows networking feature in any Cargo.toml (GC-08, D-057)"
hits="$(find crates apps fuzz -name Cargo.toml -not -path '*/target/*' 2>/dev/null \
  | xargs -r grep -nE 'package[[:space:]]*=[[:space:]]*"(ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)"|Win32_Networking|WinHttp|WinINet' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: network dependency or feature in a manifest:"; echo "$hits"; exit 1; }

echo "== network APIs only in update.rs and ai_net.rs (GC-08, D-057, D-094)"
hits="$(rs_files | grep -vxE 'apps/desktop/src/(update|ai_net)\.rs' \
  | xargs -r grep -nE '\b(ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)::|\b(use|extern crate) (ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)\b|\b(mio|tokio)::net\b|\bstd::net\b|\b(TcpStream|TcpListener|UdpSocket)\b|\bWinHttp|\bWinINet|\bWin32::Networking' | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: network API outside update.rs and ai_net.rs:"; echo "$hits"; exit 1; }

echo "== no window-title capture (SR-11)"
hits="$(rs_files | xargs -r grep -nE '\b(GetWindowText(W|A|Length[A-Za-z]*)?|InternalGetWindowText)\b|\bWM_GETTEXT(LENGTH)?\b|_NET_WM_NAME|\bkCGWindowName\b' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: window title API:"; echo "$hits"; exit 1; }

echo "== SQLite connections keep temporary data in memory (D-073, D-077)"
# Twin of the rule in check.ps1. Test code (tests.rs, tests/, #[cfg(test)] modules: an inline one up to its closing brace, `mod x;` just its line) is skipped.
ts_fixture_bad="$(awk -f scripts/temp-store-check.awk scripts/fixtures/temp-store-bad.rs)"
ts_fixture_good="$(awk -f scripts/temp-store-check.awk scripts/fixtures/temp-store-good.rs)"
[ "$ts_fixture_bad" = "scripts/fixtures/temp-store-bad.rs:3" ] || fail "temp-store self-test: bad fixture not flagged exactly once (got: $ts_fixture_bad)"
[ -z "$ts_fixture_good" ] || fail "temp-store self-test: good fixture flagged ($ts_fixture_good)"
for ts_f in after-inline after-external; do
  ts_out="$(awk -f scripts/temp-store-check.awk "scripts/fixtures/temp-store-bad-$ts_f.rs")"
  [ -n "$ts_out" ] && [ "$(printf '%s
' "$ts_out" | wc -l | tr -d ' ')" = 1 ] || fail "temp-store self-test: bad-$ts_f fixture not flagged exactly once (got: $ts_out)"
done
echo "self-test ok: bad fixture flagged ($ts_fixture_bad), good fixture passed"
hits="$(rs_files | grep -vE '(^|/)tests\.rs$|/tests/' | xargs -r awk -f scripts/temp-store-check.awk || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: SQLite connection without keep_temp_in_memory (or '// temp-store: <reason>'):"; echo "$hits"; exit 1; }

echo "== mandatory markers present (GC-07)"
mf=crates/darkclip-core/src/markers.rs
if [ -f "$mf" ]; then
  for n in ExcludeClipboardContentFromMonitorProcessing CanIncludeInClipboardHistory org.nspasteboard.ConcealedType x-kde-passwordManagerHint; do
    grep -qF "$n" "$mf" || fail "mandatory marker '$n' missing from $mf"
  done
fi

echo "== JSON schemas and examples"
command -v node >/dev/null || fail "node not found (required for schema validation)"
[ -d tools/schema-check/node_modules ] || (cd tools/schema-check && npm ci --silent) || fail "npm ci in tools/schema-check"
step "schema validation" node tools/schema-check/validate.mjs
step "workflow YAML parses" node tools/schema-check/check-workflows.mjs

echo "== required Phase 0 documents"
for p in docs/product/product-vision.md docs/product/feature-matrix.md docs/product/mvp-scope.md docs/product/user-journeys.md \
  docs/architecture/system-overview.md docs/architecture/platform-compatibility.md docs/architecture/data-model.md \
  docs/architecture/ai-integration.md docs/security/threat-model.md docs/security/privacy-design.md \
  docs/plans/roadmap.md docs/plans/global-constraints.md memory-bank/activeContext.md memory-bank/progress.md CLAUDE.md AGENTS.md; do
  [ -f "$p" ] || fail "missing $p"
done
[ "$(ls docs/adr/0*.md | wc -l)" -ge 6 ] || fail "expected at least 6 ADRs in docs/adr"

echo "== relative links in docs resolve"
broken=0
while IFS= read -r md; do
  dir="$(dirname "$md")"
  while IFS= read -r target; do
    case "$target" in http*|mailto:*) continue ;; esac
    [ -e "$dir/$target" ] || { echo "CHECK FAILED: broken link in $md: $target"; broken=1; }
  done < <(grep -oE '\]\([^)#[:space:]]+' "$md" | sed 's/^](//')
done < <(find docs memory-bank .claude -name '*.md'; printf '%s\n' README.md CLAUDE.md AGENTS.md)
[ "$broken" = 0 ] || exit 1

echo "ALL CHECKS PASSED"
