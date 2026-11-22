#!/usr/bin/env bash
# Quality gate for macOS/Linux (twin of scripts/check.ps1; keep in sync).
# Prints "ALL CHECKS PASSED" and exits 0 when everything holds.
set -u
cd "$(dirname "$0")/.."

fail() { echo "CHECK FAILED: $1"; exit 1; }
step() { local name="$1"; shift; echo "== $name"; "$@" || fail "$name"; }

has_ws=0; [ -f Cargo.toml ] && has_ws=1

# TASK-001: remove this skip block when the workspace exists.
if [ "$has_ws" = 0 ]; then echo "== cargo gates: SKIPPED (no workspace yet; Task 001 creates it)"; fi
# END TASK-001

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
fi

rs_files() { find crates apps -name '*.rs' -not -path '*/target/*' 2>/dev/null; }

echo "== no logging in privacy-critical modules (GC-06)"
hits="$(rs_files | grep -E '^(crates/darkclip-app/src/capture\.rs|crates/darkclip-core/src/policy\.rs|crates/darkclip-core/src/sensitive/.*\.rs)$' \
  | xargs -r grep -nE '\b(e?println|e?print|dbg)!|\b(log|tracing)::(trace|debug|info|warn|error)!|\b(trace|debug|info|warn|error)!\s*\(' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: logging macro:"; echo "$hits"; exit 1; }

echo "== clipboard read/write only from the capture actor (GC-03)"
hits="$(rs_files | grep -vE '^(crates/darkclip-app/src/capture\.rs|crates/darkclip-platform/src/.*|crates/darkclip-(windows|macos|linux)/src/.*)$' \
  | xargs -r grep -nE '\b(access|acc|clipboard_access)\s*\.\s*(read|write)\s*\(' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: clipboard access outside capture actor:"; echo "$hits"; exit 1; }

echo "== no window-title capture (SR-11)"
hits="$(rs_files | xargs -r grep -nE '\bGetWindowText(W|A|Length[A-Za-z]*)?\b|_NET_WM_NAME|\bkCGWindowName\b' || true)"
[ -z "$hits" ] || { echo "CHECK FAILED: window title API:"; echo "$hits"; exit 1; }

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
